use crate::binding::*;
use network::network::{JobSession2P, JobSessionMP};
use std::ffi::{c_void, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::slice;

// CGO generates separate C types for each Go package, so we need the conversion functions.
fn cjob(job: &JobSession2P) -> *mut network::binding::JOB_SESSION_2P_PTR {
    job.get_c_job()
}

fn cjobmp(job: &JobSessionMP) -> *mut network::binding::JOB_SESSION_MP_PTR {
    job.get_c_job()
}

// ---------- Utility functions for memory management ----------

pub struct CMem {
    inner: cmem_t,
}

impl CMem {
    pub fn new(data: &[u8]) -> Self {
        CMem {
            inner: cmem_t {
                data: if data.is_empty() {
                    ptr::null_mut()
                } else {
                    data.as_ptr() as *mut u8
                },
                size: data.len() as c_int,
            },
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        if self.inner.data.is_null() {
            return Vec::new();
        }
        unsafe { slice::from_raw_parts(self.inner.data, self.inner.size as usize).to_vec() }
    }
}

pub struct CMems {
    inner: cmems_t,
}

impl CMems {
    pub fn new(data: &[Vec<u8>]) -> Self {
        let count = data.len();
        if count == 0 {
            return CMems {
                inner: cmems_t {
                    count: 0,
                    data: ptr::null_mut(),
                    sizes: ptr::null_mut(),
                },
            };
        }

        let total_size: usize = data.iter().map(|v| v.len()).sum();
        let mut flat_data = Vec::with_capacity(total_size);
        let mut sizes = Vec::with_capacity(count);

        for item in data {
            sizes.push(item.len() as c_int);
            flat_data.extend_from_slice(item);
        }

        CMems {
            inner: cmems_t {
                count: count as c_int,
                data: flat_data.as_ptr() as *mut u8,
                sizes: sizes.as_ptr() as *mut c_int,
            },
        }
    }

    pub fn to_vec(&self) -> Vec<Vec<u8>> {
        if self.inner.count == 0 || self.inner.data.is_null() {
            return Vec::new();
        }

        unsafe {
            let sizes = slice::from_raw_parts(self.inner.sizes, self.inner.count as usize);
            let mut result = Vec::with_capacity(self.inner.count as usize);
            let mut offset = 0;

            for &size in sizes {
                let size = size as usize;
                let slice = slice::from_raw_parts(self.inner.data.add(offset), size);
                result.push(slice.to_vec());
                offset += size;
            }

            result
        }
    }
}

// ---------- Main library implementation ----------

pub struct Ecdsa2pKey {
    ptr: *mut MPC_ECDSA2PC_KEY_PTR,
}

impl Ecdsa2pKey {
    pub fn distributed_key_gen(job: &JobSession2P, curve_code: i32) -> Result<Self, String> {
        let mut key_ptr = ptr::null_mut();
        let err = unsafe { mpc_ecdsa2p_dkg(cjob(job), curve_code as c_int, key_ptr) };
        if err != 0 {
            return Err(format!("ECDSA-2p keygen failed with error code {}", err));
        }
        Ok(Ecdsa2pKey { ptr: key_ptr })
    }

    pub fn refresh(&self, job: &JobSession2P) -> Result<Self, String> {
        let mut new_key_ptr = ptr::null_mut();
        let err = unsafe { mpc_ecdsa2p_refresh(cjob(job), self.ptr, new_key_ptr) };
        if err != 0 {
            return Err(format!("ECDSA-2p refresh failed with error code {}", err));
        }
        Ok(Ecdsa2pKey { ptr: new_key_ptr })
    }

    pub fn sign(
        &self,
        job: &JobSession2P,
        session_id: &[u8],
        msgs: &[Vec<u8>],
    ) -> Result<Vec<Vec<u8>>, String> {
        let cmem_session = CMem::new(session_id);
        let cmems_msgs = CMems::new(msgs);
        let mut sigs = cmems_t {
            count: 0,
            data: ptr::null_mut(),
            sizes: ptr::null_mut(),
        };

        let err = unsafe {
            mpc_ecdsa2p_sign(
                cjob(job),
                cmem_session.inner,
                self.ptr,
                cmems_msgs.inner,
                &mut sigs,
            )
        };

        if err != 0 {
            return Err(format!("ECDSA-2p sign failed with error code {}", err));
        }

        let result = CMems { inner: sigs }.to_vec();
        Ok(result)
    }
}

impl Drop for Ecdsa2pKey {
    fn drop(&mut self) {
        unsafe {
            free_mpc_ecdsa2p_key_wrapper(*self.ptr);
        }
    }
}

pub struct EcdsaMpcKey {
    ptr: *mut MPC_ECDSAMPC_KEY_PTR,
}

impl EcdsaMpcKey {
    pub fn distributed_key_gen(job: &JobSessionMP, curve_code: i32) -> Result<Self, String> {
        let mut key_ptr = ptr::null_mut();
        let err = unsafe { mpc_ecdsampc_dkg(cjobmp(job), curve_code as c_int, key_ptr) };
        if err != 0 {
            return Err(format!("ECDSA-MPC keygen failed with error code {}", err));
        }
        Ok(EcdsaMpcKey { ptr: key_ptr })
    }

    pub fn sign(
        &self,
        job: &JobSessionMP,
        msg: &[u8],
        sig_receiver: i32,
    ) -> Result<Vec<u8>, String> {
        let cmem_msg = CMem::new(msg);
        let mut sig = cmem_t {
            data: ptr::null_mut(),
            size: 0,
        };

        let err = unsafe {
            mpc_ecdsampc_sign(
                cjobmp(job),
                self.ptr,
                cmem_msg.inner,
                sig_receiver as c_int,
                &mut sig,
            )
        };

        if err != 0 {
            return Err(format!("ECDSA-MPC sign failed with error code {}", err));
        }

        let result = CMem { inner: sig }.to_bytes();
        Ok(result)
    }

    pub fn public_key_to_string(&self) -> Result<(Vec<u8>, Vec<u8>), String> {
        let mut x_mem = cmem_t {
            data: ptr::null_mut(),
            size: 0,
        };
        let mut y_mem = cmem_t {
            data: ptr::null_mut(),
            size: 0,
        };

        let err = unsafe { ecdsa_mpc_public_key_to_string(self.ptr, &mut x_mem, &mut y_mem) };

        if err != 0 {
            return Err(format!(
                "Failed to get public key string with error code {}",
                err
            ));
        }

        let x = CMem { inner: x_mem }.to_bytes();
        let y = CMem { inner: y_mem }.to_bytes();
        Ok((x, y))
    }
}

impl Drop for EcdsaMpcKey {
    fn drop(&mut self) {
        unsafe {
            free_mpc_ecdsamp_key_wrapper(*self.ptr);
        }
    }
}

pub struct SsNode {
    ptr: *mut CRYPTO_SS_NODE_PTR,
}

impl SsNode {
    pub fn new(node_type: i32, node_name: &str, threshold: i32) -> Self {
        let mut ptr = ptr::null_mut();
        let c_name = CString::new(node_name).unwrap();
        unsafe {
            *ptr = new_node(
                node_type as c_int,
                cmem_t {
                    data: c_name.as_ptr() as *mut u8,
                    size: c_name.as_bytes().len() as c_int,
                },
                threshold as c_int,
            );
        };
        SsNode { ptr }
    }

    pub fn add_child(&self, child: &SsNode) {
        unsafe {
            add_child(self.ptr, child.ptr);
        }
    }

    pub fn pve_quorum_encrypt(
        &self,
        public_keys: &[Vec<u8>],
        xs: &[Vec<u8>],
        label: &str,
    ) -> Result<Vec<u8>, String> {
        let c_label = CString::new(label).unwrap();
        let mut out = cmem_t {
            data: ptr::null_mut(),
            size: 0,
        };

        let err = unsafe {
            pve_quorum_encrypt(
                self.ptr,
                CMems::new(public_keys).inner,
                public_keys.len() as c_int,
                CMems::new(xs).inner,
                xs.len() as c_int,
                c_label.as_ptr(),
                &mut out,
            )
        };

        if err != 0 {
            return Err(format!("PVE quorum encrypt failed with error code {}", err));
        }

        Ok(CMem { inner: out }.to_bytes())
    }

    pub fn pve_quorum_decrypt(
        &self,
        private_keys: &[Vec<u8>],
        public_keys: &[Vec<u8>],
        pve_bundle: &[u8],
        xs: &[Vec<u8>],
        label: &str,
    ) -> Result<Vec<Vec<u8>>, String> {
        let c_label = CString::new(label).unwrap();
        let mut out = cmems_t {
            count: 0,
            data: ptr::null_mut(),
            sizes: ptr::null_mut(),
        };

        let err = unsafe {
            pve_quorum_decrypt(
                self.ptr,
                CMems::new(private_keys).inner,
                private_keys.len() as c_int,
                CMems::new(public_keys).inner,
                public_keys.len() as c_int,
                CMem::new(pve_bundle).inner,
                CMems::new(xs).inner,
                xs.len() as c_int,
                c_label.as_ptr(),
                &mut out,
            )
        };

        if err != 0 {
            return Err(format!("PVE quorum decrypt failed with error code {}", err));
        }

        Ok(CMems { inner: out }.to_vec())
    }
}

impl Drop for SsNode {
    fn drop(&mut self) {
        unsafe {
            free_crypto_ss_node_wrapper(*self.ptr);
        }
    }
}

pub fn zk_dl_example() -> i32 {
    unsafe { ZK_DL_Example() as i32 }
}

pub fn new_enc_key_pairs(count: i32) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>), String> {
    let mut private_keys = cmems_t {
        count: 0,
        data: ptr::null_mut(),
        sizes: ptr::null_mut(),
    };
    let mut public_keys = cmems_t {
        count: 0,
        data: ptr::null_mut(),
        sizes: ptr::null_mut(),
    };

    let err = unsafe { get_n_enc_keypairs(count as c_int, &mut private_keys, &mut public_keys) };

    if err != 0 {
        return Err(format!(
            "Failed to generate key pairs with error code {}",
            err
        ));
    }

    let priv_keys = CMems {
        inner: private_keys,
    }
    .to_vec();
    let pub_keys = CMems { inner: public_keys }.to_vec();
    Ok((priv_keys, pub_keys))
}

pub fn new_ec_key_pairs(count: i32) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>), String> {
    let mut private_keys = cmems_t {
        count: 0,
        data: ptr::null_mut(),
        sizes: ptr::null_mut(),
    };
    let mut public_keys = cmems_t {
        count: 0,
        data: ptr::null_mut(),
        sizes: ptr::null_mut(),
    };

    let err = unsafe { get_n_ec_keypairs(count as c_int, &mut private_keys, &mut public_keys) };

    if err != 0 {
        return Err(format!(
            "Failed to generate EC key pairs with error code {}",
            err
        ));
    }

    let priv_keys = CMems {
        inner: private_keys,
    }
    .to_vec();
    let pub_keys = CMems { inner: public_keys }.to_vec();
    Ok((priv_keys, pub_keys))
}


pub fn serialize_ecdsa_shares(keyshares: Vec<MPC_ECDSAMPC_KEY_PTR>) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>), String> {
    let mut xs = Vec::with_capacity(keyshares.len());
    let mut qs = Vec::with_capacity(keyshares.len());

    for  mut keyshare in keyshares {
        let mut x_mem = cmem_t {
            data: ptr::null_mut(),
            size: 0,
        };
        let mut q_mem = cmem_t {
            data: ptr::null_mut(),
            size: 0,
        };

        let err = unsafe { convert_ecdsa_share_to_bn_t_share(*keyshare, &mut x_mem, &mut q_mem) };

        if err != 0 {
            return Err(format!("Failed to serialize ECDSA shares with error code {}", err));
        }

        xs.push(CMem { inner: x_mem }.to_bytes());
        qs.push(CMem { inner: q_mem }.to_bytes());
    }

    Ok((xs, qs))
}