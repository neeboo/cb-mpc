include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::sync::{Mutex, Arc};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use lazy_static::lazy_static;


/// Trait for data transport implementations
pub trait IDataTransport {
    fn message_send(&self, receiver: i32, buffer: &[u8]) -> Result<(), String>;
    fn message_receive(&self, sender: i32) -> Result<Vec<u8>, String>;
    fn messages_receive(&self, senders: &[i32]) -> Result<Vec<Vec<u8>>, String>;
}

// Global storage for IDataTransport implementations
lazy_static::lazy_static! {
    static ref DT_IMPL_MAP: Mutex<HashMap<usize, Arc<dyn IDataTransport + Send + Sync>>> = Mutex::new(HashMap::new());
}

pub fn set_dt_impl(dt_impl: Arc<dyn IDataTransport + Send + Sync>) -> *mut c_void {
    let ptr = Box::into_raw(Box::new(())) as *mut c_void;
    DT_IMPL_MAP.lock().unwrap().insert(ptr as usize, dt_impl);
    ptr
}

fn get_dt_impl(ptr: *mut c_void) -> Arc<dyn IDataTransport + Send + Sync> {
    DT_IMPL_MAP.lock().unwrap().get(&(ptr as usize)).unwrap().clone()
}

pub fn free_dt_impl(ptr: *mut c_void) {
    DT_IMPL_MAP.lock().unwrap().remove(&(ptr as usize));
    unsafe { Box::from_raw(ptr); }
}

/// Initialize callbacks
pub fn init_callbacks() {
    unsafe {
        let mut callbacks = data_transport_callbacks_t {
            send_fun: Some(callback_send),
            receive_fun: Some(callback_receive),
            receive_all_fun: Some(callback_receive_all),
        };
        set_callbacks_wrapper(&mut callbacks);
    }
}

/// JobSession2P wrapper
pub struct JobSession2P {
    dt_impl_ptr: *mut c_void,
    c_job: *mut JOB_SESSION_2P_PTR,
}

impl JobSession2P {
    pub fn new(dt_impl: Arc<dyn IDataTransport + Send + Sync>, role_index: i32) -> Self {
        let ptr = set_dt_impl(dt_impl);
        let c_job = unsafe { new_job_session_2p(ptr::null_mut(), ptr, role_index) };
        Self { dt_impl_ptr: ptr, c_job }
    }

    pub fn is_peer1(&self) -> bool {
        unsafe { is_peer1(self.c_job) != 0 }
    }

    pub fn is_peer2(&self) -> bool {
        unsafe { is_peer2(self.c_job) != 0 }
    }

    pub fn is_role_index(&self, role_index: i32) -> bool {
        unsafe { is_role_index(self.c_job, role_index) != 0 }
    }

    pub fn get_role_index(&self) -> i32 {
        unsafe { get_role_index(self.c_job) }
    }

    pub fn message(&self, sender: i32, receiver: i32, msg: &[u8]) -> Result<Vec<u8>, String> {
        if self.is_role_index(sender) {
            unsafe {
                let result = mpc_2p_send(self.c_job, receiver, msg.as_ptr(), msg.len() as i32);
                if result != 0 {
                    return Err("2p send failed".to_string());
                }
                Ok(msg.to_vec())
            }
        } else if self.is_role_index(receiver) {
            unsafe {
                let mut message_ptr: *mut u8 = ptr::null_mut();
                let mut message_size: i32 = 0;
                let result = mpc_2p_receive(self.c_job, sender, &mut message_ptr, &mut message_size);
                if result != 0 {
                    return Err("2p receive failed".to_string());
                }
                let data = Vec::from_raw_parts(message_ptr, message_size as usize, message_size as usize);
                Ok(data)
            }
        } else {
            Err("caller needs to be a sender or receiver".to_string())
        }
    }
    pub fn get_c_job(&self) -> *mut JOB_SESSION_2P_PTR {
        self.c_job
    }
}

impl Drop for JobSession2P {
    fn drop(&mut self) {
        unsafe {
            free_job_session_2p_wrapper(self.c_job);
            free_dt_impl(self.dt_impl_ptr);
        }
    }
}

/// JobSessionMP wrapper
pub struct JobSessionMP {
    dt_impl_ptr: *mut c_void,
    c_job: *mut JOB_SESSION_MP_PTR,
}

impl JobSessionMP {
    pub fn new(dt_impl: Arc<dyn IDataTransport + Send + Sync>, party_count: i32, role_index: i32, job_session_id: i32) -> Self {
        let ptr = set_dt_impl(dt_impl);
        let c_job = unsafe { new_job_session_mp(ptr::null_mut(), ptr, party_count, role_index, job_session_id) };
        Self { dt_impl_ptr: ptr, c_job }
    }

    pub fn is_party(&self, party_index: i32) -> bool {
        unsafe { is_party(self.c_job, party_index) != 0 }
    }

    pub fn get_party_index(&self) -> i32 {
        unsafe { get_party_idx(self.c_job) }
    }

    pub fn get_c_job(&self) -> *mut JOB_SESSION_MP_PTR {
        self.c_job
    }
}

impl Drop for JobSessionMP {
    fn drop(&mut self) {
        unsafe {
            free_job_session_mp_wrapper(self.c_job);
            free_dt_impl(self.dt_impl_ptr);
        }
    }
}

/// Helper function to convert CMEM to Vec<u8>
pub fn cmem_get(cmem: cmem_t) -> Vec<u8> {
    if cmem.data.is_null() {
        return Vec::new();
    }
    unsafe {
        let data = Vec::from_raw_parts(cmem.data, cmem.size as usize, cmem.size as usize);
        data
    }
}

/// Agree on random value
pub fn agree_random(job: &JobSession2P, bit_len: i32) -> Result<Vec<u8>, String> {
    unsafe {
        let mut out = cmem_t { data: ptr::null_mut(), size: 0 };
        let result = mpc_agree_random(job.c_job, bit_len, &mut out);
        if result != 0 {
            return Err("mpc_agree_random failed".to_string());
        }
        Ok(cmem_get(out))
    }
}

// Initialize callbacks when module is loaded
#[ctor::ctor]
fn init() {
    init_callbacks();
}
