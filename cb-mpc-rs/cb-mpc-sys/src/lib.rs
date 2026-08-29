//! C ABI, allocator, and callback boundary for Coinbase cb-mpc.

use std::ffi::CString;
use std::fmt;
use std::marker::PhantomData;
use std::os::raw::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

#[allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use bindings::{cmem_t as RawCmem, cmems_t as RawCmems};

/// Errors detected at the Rust/C ownership boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InputTooLarge,
    InvalidCBuffer,
    InvalidArgument,
    AllocationFailed,
    C(i32),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => formatter.write_str("input exceeds the C API's i32 size limit"),
            Self::InvalidCBuffer => formatter.write_str("the C API returned an invalid buffer"),
            Self::InvalidArgument => formatter.write_str("invalid cb-mpc argument"),
            Self::AllocationFailed => formatter.write_str("cb-mpc allocator returned null"),
            Self::C(code) => write!(formatter, "cb-mpc C API error {code:#x}"),
        }
    }
}

impl std::error::Error for Error {}

const CBMPC_SUCCESS: i32 = 0;
const CBMPC_E_GENERAL: i32 = 0xff01_0001_u32 as i32;
const CBMPC_E_BADARG: i32 = 0xff01_0002_u32 as i32;
const CBMPC_E_INSUFFICIENT: i32 = 0xff01_000c_u32 as i32;
const CBMPC_E_RANGE: i32 = 0xff01_0012_u32 as i32;

/// An error returned by a user-supplied transport implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportError {
    code: i32,
}

impl TransportError {
    pub fn new(code: i32) -> Self {
        Self {
            code: if code == CBMPC_SUCCESS {
                CBMPC_E_GENERAL
            } else {
                code
            },
        }
    }

    pub fn code(self) -> i32 {
        self.code
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "transport error {:#x}", self.code)
    }
}

impl std::error::Error for TransportError {}

/// Synchronous message transport used by cb-mpc interactive protocols.
///
/// Implementations must be safe to call from the protocol thread. Every method
/// is invoked behind a panic barrier before control returns to C++.
pub trait Transport: Send + Sync {
    fn send(&self, receiver: i32, data: &[u8]) -> Result<(), TransportError>;
    fn receive(&self, sender: i32) -> Result<Vec<u8>, TransportError>;
    fn receive_all(&self, senders: &[i32]) -> Result<Vec<Vec<u8>>, TransportError>;
}

struct TransportContext<'a> {
    transport: &'a dyn Transport,
}

impl<'a> TransportContext<'a> {
    fn new(transport: &'a dyn Transport) -> Self {
        Self { transport }
    }
}

fn ffi_result(operation: impl FnOnce() -> Result<(), TransportError>) -> i32 {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => CBMPC_SUCCESS,
        Ok(Err(error)) => error.code(),
        Err(_) => CBMPC_E_GENERAL,
    }
}

unsafe fn transport_from_ctx<'a>(ctx: *mut c_void) -> Result<&'a dyn Transport, TransportError> {
    if ctx.is_null() {
        return Err(TransportError::new(CBMPC_E_BADARG));
    }
    let context = unsafe { &*ctx.cast::<TransportContext<'a>>() };
    Ok(context.transport)
}

unsafe extern "C" fn transport_send(
    ctx: *mut c_void,
    receiver: i32,
    data: *const u8,
    size: i32,
) -> i32 {
    ffi_result(|| {
        if size < 0 || (size > 0 && data.is_null()) {
            return Err(TransportError::new(CBMPC_E_BADARG));
        }
        let transport = unsafe { transport_from_ctx(ctx)? };
        let message = if size == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(data, size as usize) }
        };
        transport.send(receiver, message)
    })
}

unsafe extern "C" fn transport_receive(
    ctx: *mut c_void,
    sender: i32,
    out_msg: *mut RawCmem,
) -> i32 {
    ffi_result(|| {
        if out_msg.is_null() {
            return Err(TransportError::new(CBMPC_E_BADARG));
        }
        unsafe { out_msg.write(RawCmem::default()) };
        let transport = unsafe { transport_from_ctx(ctx)? };
        let message = transport.receive(sender)?;
        let size = i32::try_from(message.len()).map_err(|_| TransportError::new(CBMPC_E_RANGE))?;
        if message.is_empty() {
            return Ok(());
        }
        let data = unsafe { bindings::cbmpc_malloc(message.len()) }.cast::<u8>();
        if data.is_null() {
            return Err(TransportError::new(CBMPC_E_INSUFFICIENT));
        }
        unsafe {
            ptr::copy_nonoverlapping(message.as_ptr(), data, message.len());
            out_msg.write(RawCmem { data, size });
        }
        Ok(())
    })
}

unsafe extern "C" fn transport_receive_all(
    ctx: *mut c_void,
    senders: *const i32,
    senders_count: i32,
    out_msgs: *mut RawCmems,
) -> i32 {
    ffi_result(|| {
        if out_msgs.is_null() || senders_count < 0 || (senders_count > 0 && senders.is_null()) {
            return Err(TransportError::new(CBMPC_E_BADARG));
        }
        unsafe { out_msgs.write(RawCmems::default()) };
        let sender_slice = if senders_count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(senders, senders_count as usize) }
        };
        let transport = unsafe { transport_from_ctx(ctx)? };
        let messages = transport.receive_all(sender_slice)?;
        if messages.len() != sender_slice.len() {
            return Err(TransportError::new(CBMPC_E_GENERAL));
        }

        let mut sizes = Vec::with_capacity(messages.len());
        let mut total = 0_usize;
        for message in &messages {
            sizes.push(
                i32::try_from(message.len()).map_err(|_| TransportError::new(CBMPC_E_RANGE))?,
            );
            total = total
                .checked_add(message.len())
                .ok_or_else(|| TransportError::new(CBMPC_E_RANGE))?;
            i32::try_from(total).map_err(|_| TransportError::new(CBMPC_E_RANGE))?;
        }

        let sizes_bytes = sizes
            .len()
            .checked_mul(std::mem::size_of::<i32>())
            .ok_or_else(|| TransportError::new(CBMPC_E_RANGE))?;
        let sizes_ptr = if sizes.is_empty() {
            ptr::null_mut()
        } else {
            let allocation = unsafe { bindings::cbmpc_malloc(sizes_bytes) }.cast::<i32>();
            if allocation.is_null() {
                return Err(TransportError::new(CBMPC_E_INSUFFICIENT));
            }
            unsafe { ptr::copy_nonoverlapping(sizes.as_ptr(), allocation, sizes.len()) };
            allocation
        };

        let data_ptr = if total == 0 {
            ptr::null_mut()
        } else {
            let allocation = unsafe { bindings::cbmpc_malloc(total) }.cast::<u8>();
            if allocation.is_null() {
                unsafe { bindings::cbmpc_free(sizes_ptr.cast()) };
                return Err(TransportError::new(CBMPC_E_INSUFFICIENT));
            }
            let mut offset = 0_usize;
            for message in &messages {
                unsafe {
                    ptr::copy_nonoverlapping(
                        message.as_ptr(),
                        allocation.add(offset),
                        message.len(),
                    )
                };
                offset += message.len();
            }
            allocation
        };

        unsafe {
            out_msgs.write(RawCmems {
                count: senders_count,
                data: data_ptr,
                sizes: sizes_ptr,
            })
        };
        Ok(())
    })
}

unsafe extern "C" fn transport_free(_: *mut c_void, allocation: *mut c_void) {
    if !allocation.is_null() {
        unsafe { bindings::cbmpc_free(allocation) };
    }
}

struct MpJob<'a> {
    raw: bindings::cbmpc_mp_job_t,
    _context: Box<TransportContext<'a>>,
    _transport: Box<bindings::cbmpc_transport_t>,
    _party_names: Vec<CString>,
    _party_name_ptrs: Vec<*const c_char>,
}

impl<'a> MpJob<'a> {
    fn new(
        self_index: usize,
        party_names: &[String],
        transport: &'a dyn Transport,
    ) -> Result<Self, Error> {
        if party_names.is_empty() || self_index >= party_names.len() {
            return Err(Error::InvalidArgument);
        }
        let count = i32::try_from(party_names.len()).map_err(|_| Error::InputTooLarge)?;
        let party_names = party_names
            .iter()
            .map(|name| CString::new(name.as_str()).map_err(|_| Error::InvalidArgument))
            .collect::<Result<Vec<_>, _>>()?;
        let party_name_ptrs = party_names
            .iter()
            .map(|name| name.as_ptr())
            .collect::<Vec<_>>();
        let mut context = Box::new(TransportContext::new(transport));
        let transport_table = Box::new(bindings::cbmpc_transport_t {
            ctx: (&mut *context as *mut TransportContext<'a>).cast(),
            send: Some(transport_send),
            receive: Some(transport_receive),
            receive_all: Some(transport_receive_all),
            free: Some(transport_free),
        });
        let raw = bindings::cbmpc_mp_job_t {
            self_: self_index as i32,
            party_names: party_name_ptrs.as_ptr(),
            party_names_count: count,
            transport: &*transport_table,
        };
        Ok(Self {
            raw,
            _context: context,
            _transport: transport_table,
            _party_names: party_names,
            _party_name_ptrs: party_name_ptrs,
        })
    }

    fn as_raw(&self) -> &bindings::cbmpc_mp_job_t {
        &self.raw
    }
}

struct CNames {
    _names: Vec<CString>,
    pointers: Vec<*const c_char>,
}

impl CNames {
    fn new(names: &[String]) -> Result<Self, Error> {
        let names = names
            .iter()
            .map(|name| CString::new(name.as_str()).map_err(|_| Error::InvalidArgument))
            .collect::<Result<Vec<_>, _>>()?;
        let pointers = names.iter().map(|name| name.as_ptr()).collect();
        Ok(Self {
            _names: names,
            pointers,
        })
    }

    fn as_ptr(&self) -> *const *const c_char {
        if self.pointers.is_empty() {
            ptr::null()
        } else {
            self.pointers.as_ptr()
        }
    }

    fn count(&self) -> Result<i32, Error> {
        i32::try_from(self.pointers.len()).map_err(|_| Error::InputTooLarge)
    }
}

struct ThresholdAccessStructure {
    raw: bindings::cbmpc_access_structure_t,
    _leaf_names: Vec<CString>,
    nodes: Vec<bindings::cbmpc_access_structure_node_t>,
    child_indices: Vec<i32>,
}

impl ThresholdAccessStructure {
    fn new(threshold: usize, party_names: &[String]) -> Result<Self, Error> {
        if threshold == 0 || threshold > party_names.len() || party_names.is_empty() {
            return Err(Error::InvalidArgument);
        }
        let threshold = i32::try_from(threshold).map_err(|_| Error::InputTooLarge)?;
        let party_count = i32::try_from(party_names.len()).map_err(|_| Error::InputTooLarge)?;
        let leaf_names = party_names
            .iter()
            .map(|name| CString::new(name.as_str()).map_err(|_| Error::InvalidArgument))
            .collect::<Result<Vec<_>, _>>()?;
        let mut nodes = Vec::with_capacity(leaf_names.len() + 1);
        nodes.push(bindings::cbmpc_access_structure_node_t {
            type_:
                bindings::cbmpc_access_structure_node_type_e_CBMPC_ACCESS_STRUCTURE_NODE_THRESHOLD,
            leaf_name: ptr::null(),
            threshold_k: threshold,
            child_indices_offset: 0,
            child_indices_count: party_count,
        });
        nodes.extend(
            leaf_names
                .iter()
                .map(|name| bindings::cbmpc_access_structure_node_t {
                    type_: bindings::cbmpc_access_structure_node_type_e_CBMPC_ACCESS_STRUCTURE_NODE_LEAF,
                    leaf_name: name.as_ptr(),
                    threshold_k: 0,
                    child_indices_offset: 0,
                    child_indices_count: 0,
                }),
        );
        let child_indices = (1..=party_count).collect::<Vec<_>>();
        let raw = bindings::cbmpc_access_structure_t {
            nodes: nodes.as_ptr(),
            nodes_count: i32::try_from(nodes.len()).map_err(|_| Error::InputTooLarge)?,
            child_indices: child_indices.as_ptr(),
            child_indices_count: party_count,
            root_index: 0,
        };
        Ok(Self {
            raw,
            _leaf_names: leaf_names,
            nodes,
            child_indices,
        })
    }

    fn as_raw(&self) -> &bindings::cbmpc_access_structure_t {
        debug_assert_eq!(self.raw.nodes, self.nodes.as_ptr());
        debug_assert_eq!(self.raw.child_indices, self.child_indices.as_ptr());
        &self.raw
    }
}

/// Run one party's threshold ECDSA DKG call.
pub fn ecdsa_mp_dkg_threshold(
    self_index: usize,
    party_names: &[String],
    threshold: usize,
    quorum_party_names: &[String],
    transport: &dyn Transport,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let job = MpJob::new(self_index, party_names, transport)?;
    let access_structure = ThresholdAccessStructure::new(threshold, party_names)?;
    let quorum = CNames::new(quorum_party_names)?;
    let empty_sid = BorrowedCmem::new(&[])?;
    let mut out_key = OutCmem::new();
    let mut out_sid = OutCmem::new();
    let code = unsafe {
        bindings::cbmpc_ecdsa_mp_dkg_ac(
            job.as_raw(),
            bindings::cbmpc_curve_id_e_CBMPC_CURVE_SECP256K1,
            empty_sid.as_raw(),
            access_structure.as_raw(),
            quorum.as_ptr(),
            quorum.count()?,
            out_key.as_mut_ptr(),
            out_sid.as_mut_ptr(),
        )
    };
    let key = out_key.into_result(code)?;
    let sid = out_sid.into_result(code)?;
    Ok((key.to_vec(), sid.to_vec()))
}

/// Run one party's threshold ECDSA refresh call.
pub fn ecdsa_mp_refresh_threshold(
    self_index: usize,
    party_names: &[String],
    threshold: usize,
    quorum_party_names: &[String],
    sid: &[u8],
    key_blob: &[u8],
    transport: &dyn Transport,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let job = MpJob::new(self_index, party_names, transport)?;
    let access_structure = ThresholdAccessStructure::new(threshold, party_names)?;
    let quorum = CNames::new(quorum_party_names)?;
    let sid = BorrowedCmem::new(sid)?;
    let key_blob = BorrowedCmem::new(key_blob)?;
    let mut out_sid = OutCmem::new();
    let mut out_key = OutCmem::new();
    let code = unsafe {
        bindings::cbmpc_ecdsa_mp_refresh_ac(
            job.as_raw(),
            sid.as_raw(),
            key_blob.as_raw(),
            access_structure.as_raw(),
            quorum.as_ptr(),
            quorum.count()?,
            out_sid.as_mut_ptr(),
            out_key.as_mut_ptr(),
        )
    };
    let new_sid = out_sid.into_result(code)?;
    let new_key = out_key.into_result(code)?;
    Ok((new_key.to_vec(), new_sid.to_vec()))
}

/// Run one online party's threshold ECDSA signing call.
pub fn ecdsa_mp_sign_threshold(
    self_index: usize,
    online_party_names: &[String],
    policy_party_names: &[String],
    threshold: usize,
    key_blob: &[u8],
    message_hash: &[u8],
    signature_receiver: usize,
    transport: &dyn Transport,
) -> Result<Vec<u8>, Error> {
    let job = MpJob::new(self_index, online_party_names, transport)?;
    if signature_receiver >= online_party_names.len() {
        return Err(Error::InvalidArgument);
    }
    let access_structure = ThresholdAccessStructure::new(threshold, policy_party_names)?;
    let key_blob = BorrowedCmem::new(key_blob)?;
    let message_hash = BorrowedCmem::new(message_hash)?;
    let mut out_signature = OutCmem::new();
    let code = unsafe {
        bindings::cbmpc_ecdsa_mp_sign_ac(
            job.as_raw(),
            key_blob.as_raw(),
            access_structure.as_raw(),
            message_hash.as_raw(),
            i32::try_from(signature_receiver).map_err(|_| Error::InputTooLarge)?,
            out_signature.as_mut_ptr(),
        )
    };
    Ok(out_signature.into_result(code)?.to_vec())
}

fn single_output(operation: impl FnOnce(*mut RawCmem) -> i32) -> Result<Vec<u8>, Error> {
    let mut output = OutCmem::new();
    let code = operation(output.as_mut_ptr());
    Ok(output.into_result(code)?.to_vec())
}

pub fn ecdsa_mp_public_key_compressed(key_blob: &[u8]) -> Result<Vec<u8>, Error> {
    let key_blob = BorrowedCmem::new(key_blob)?;
    single_output(|output| unsafe {
        bindings::cbmpc_ecdsa_mp_get_public_key_compressed(key_blob.as_raw(), output)
    })
}

pub fn ecdsa_mp_public_share_compressed(key_blob: &[u8]) -> Result<Vec<u8>, Error> {
    let key_blob = BorrowedCmem::new(key_blob)?;
    single_output(|output| unsafe {
        bindings::cbmpc_ecdsa_mp_get_public_share_compressed(key_blob.as_raw(), output)
    })
}

pub fn ecdsa_mp_detach_private_scalar(key_blob: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let key_blob = BorrowedCmem::new(key_blob)?;
    let mut out_public_blob = OutCmem::new();
    let mut out_private_scalar = OutCmem::new();
    let code = unsafe {
        bindings::cbmpc_ecdsa_mp_detach_private_scalar(
            key_blob.as_raw(),
            out_public_blob.as_mut_ptr(),
            out_private_scalar.as_mut_ptr(),
        )
    };
    let public_blob = out_public_blob.into_result(code)?;
    let private_scalar = out_private_scalar.into_result(code)?;
    Ok((public_blob.to_vec(), private_scalar.to_vec()))
}

pub fn ecdsa_mp_attach_private_scalar(
    public_key_blob: &[u8],
    private_scalar: &[u8],
    public_share_compressed: &[u8],
) -> Result<Vec<u8>, Error> {
    let public_key_blob = BorrowedCmem::new(public_key_blob)?;
    let private_scalar = BorrowedCmem::new(private_scalar)?;
    let public_share_compressed = BorrowedCmem::new(public_share_compressed)?;
    single_output(|output| unsafe {
        bindings::cbmpc_ecdsa_mp_attach_private_scalar(
            public_key_blob.as_raw(),
            private_scalar.as_raw(),
            public_share_compressed.as_raw(),
            output,
        )
    })
}

/// A C input view that cannot outlive its Rust backing slice.
pub struct BorrowedCmem<'a> {
    raw: RawCmem,
    backing: &'a [u8],
}

impl<'a> BorrowedCmem<'a> {
    pub fn new(backing: &'a [u8]) -> Result<Self, Error> {
        let size = i32::try_from(backing.len()).map_err(|_| Error::InputTooLarge)?;
        let data = if backing.is_empty() {
            ptr::null_mut()
        } else {
            backing.as_ptr().cast_mut()
        };
        Ok(Self {
            raw: RawCmem { data, size },
            backing,
        })
    }

    pub fn as_raw(&self) -> RawCmem {
        self.raw
    }

    pub fn as_slice(&self) -> &[u8] {
        self.backing
    }
}

/// A flattened C list that owns both backing allocations for its whole lifetime.
pub struct BorrowedCmems {
    raw: RawCmems,
    flat_data: Vec<u8>,
    sizes: Vec<i32>,
    _not_send_across_ffi: PhantomData<*mut ()>,
}

impl BorrowedCmems {
    pub fn new(messages: &[Vec<u8>]) -> Result<Self, Error> {
        let count = i32::try_from(messages.len()).map_err(|_| Error::InputTooLarge)?;
        let mut total = 0_usize;
        let mut sizes = Vec::with_capacity(messages.len());
        for message in messages {
            sizes.push(i32::try_from(message.len()).map_err(|_| Error::InputTooLarge)?);
            total = total
                .checked_add(message.len())
                .ok_or(Error::InputTooLarge)?;
            i32::try_from(total).map_err(|_| Error::InputTooLarge)?;
        }
        let mut flat_data = Vec::with_capacity(total);
        for message in messages {
            flat_data.extend_from_slice(message);
        }

        let raw = RawCmems {
            count,
            data: if flat_data.is_empty() {
                ptr::null_mut()
            } else {
                flat_data.as_mut_ptr()
            },
            sizes: if sizes.is_empty() {
                ptr::null_mut()
            } else {
                sizes.as_mut_ptr()
            },
        };
        Ok(Self {
            raw,
            flat_data,
            sizes,
            _not_send_across_ffi: PhantomData,
        })
    }

    pub fn as_raw(&self) -> RawCmems {
        self.raw
    }

    pub fn to_vecs(&self) -> Vec<Vec<u8>> {
        let mut offset = 0_usize;
        self.sizes
            .iter()
            .map(|size| {
                let size = usize::try_from(*size).expect("sizes were validated at construction");
                let next = offset + size;
                let message = self.flat_data[offset..next].to_vec();
                offset = next;
                message
            })
            .collect()
    }
}

/// A mutable C output slot. Its address, rather than its null initial value, is passed to C.
pub struct OutCmem {
    raw: RawCmem,
}

impl OutCmem {
    pub fn new() -> Self {
        Self {
            raw: RawCmem {
                data: ptr::null_mut(),
                size: 0,
            },
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut RawCmem {
        &mut self.raw
    }

    fn into_result(mut self, code: i32) -> Result<OwnedCmem, Error> {
        let raw = std::mem::replace(
            &mut self.raw,
            RawCmem {
                data: ptr::null_mut(),
                size: 0,
            },
        );
        let owned = OwnedCmem { raw };
        if code != 0 {
            return Err(Error::C(code));
        }
        owned.validate()?;
        Ok(owned)
    }
}

impl Default for OutCmem {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OutCmem {
    fn drop(&mut self) {
        if !self.raw.data.is_null() {
            unsafe { bindings::cbmpc_cmem_free(self.raw) };
        }
    }
}

/// A buffer allocated by Coinbase cb-mpc and released by `cbmpc_cmem_free`.
pub struct OwnedCmem {
    raw: RawCmem,
}

impl OwnedCmem {
    pub fn empty() -> Self {
        Self {
            raw: RawCmem {
                data: ptr::null_mut(),
                size: 0,
            },
        }
    }

    fn validate(&self) -> Result<(), Error> {
        if self.raw.size < 0 || (self.raw.size > 0 && self.raw.data.is_null()) {
            return Err(Error::InvalidCBuffer);
        }
        Ok(())
    }

    pub fn as_slice(&self) -> &[u8] {
        if self.raw.size == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.raw.data, self.raw.size as usize) }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
}

impl Drop for OwnedCmem {
    fn drop(&mut self) {
        if !self.raw.data.is_null() {
            unsafe { bindings::cbmpc_cmem_free(self.raw) };
            self.raw.data = ptr::null_mut();
            self.raw.size = 0;
        }
    }
}

#[cfg(test)]
mod transport_tests;
