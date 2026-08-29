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
