use super::*;
use std::ffi::CStr;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingTransport {
    sent: Mutex<Vec<(i32, Vec<u8>)>>,
}

impl Transport for RecordingTransport {
    fn send(&self, receiver: i32, data: &[u8]) -> Result<(), TransportError> {
        self.sent.lock().unwrap().push((receiver, data.to_vec()));
        Ok(())
    }

    fn receive(&self, sender: i32) -> Result<Vec<u8>, TransportError> {
        Ok(vec![sender as u8, 9])
    }

    fn receive_all(&self, senders: &[i32]) -> Result<Vec<Vec<u8>>, TransportError> {
        Ok(senders.iter().map(|sender| vec![*sender as u8]).collect())
    }
}

struct PanickingTransport;

impl Transport for PanickingTransport {
    fn send(&self, _: i32, _: &[u8]) -> Result<(), TransportError> {
        panic!("panic must not cross C ABI")
    }

    fn receive(&self, _: i32) -> Result<Vec<u8>, TransportError> {
        panic!("panic must not cross C ABI")
    }

    fn receive_all(&self, _: &[i32]) -> Result<Vec<Vec<u8>>, TransportError> {
        panic!("panic must not cross C ABI")
    }
}

#[test]
fn callbacks_copy_messages_and_use_the_cbmpc_allocator() {
    let transport = RecordingTransport::default();
    let context = TransportContext::new(&transport);
    let ctx = (&context as *const TransportContext<'_>).cast_mut().cast();

    let data = [4_u8, 5, 6];
    assert_eq!(unsafe { transport_send(ctx, 2, data.as_ptr(), 3) }, 0);
    assert_eq!(
        transport.sent.lock().unwrap().as_slice(),
        &[(2, data.to_vec())]
    );

    let mut output = RawCmem::default();
    assert_eq!(unsafe { transport_receive(ctx, 7, &mut output) }, 0);
    assert_eq!(
        unsafe { std::slice::from_raw_parts(output.data, output.size as usize) },
        [7, 9]
    );
    unsafe { transport_free(ctx, output.data.cast()) };

    let senders = [1_i32, 3];
    let mut outputs = RawCmems::default();
    assert_eq!(
        unsafe { transport_receive_all(ctx, senders.as_ptr(), 2, &mut outputs) },
        0
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(outputs.sizes, 2) },
        [1, 1]
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(outputs.data, 2) },
        [1, 3]
    );
    unsafe {
        transport_free(ctx, outputs.data.cast());
        transport_free(ctx, outputs.sizes.cast());
    }
}

#[test]
fn callback_panics_become_c_errors() {
    let transport = PanickingTransport;
    let context = TransportContext::new(&transport);
    let ctx = (&context as *const TransportContext<'_>).cast_mut().cast();
    let data = [1_u8];

    let result = std::panic::catch_unwind(|| unsafe { transport_send(ctx, 0, data.as_ptr(), 1) });
    assert!(result.is_ok(), "panic crossed the extern C callback");
    assert_ne!(result.unwrap(), 0);
}

#[test]
fn mp_job_owns_names_and_points_at_a_live_transport_context() {
    let transport = RecordingTransport::default();
    let names = vec!["alice".to_owned(), "bob".to_owned(), "carol".to_owned()];
    let job = MpJob::new(1, &names, &transport).expect("valid job");
    let raw = job.as_raw();

    assert_eq!(raw.self_, 1);
    assert_eq!(raw.party_names_count, 3);
    let second = unsafe { *raw.party_names.add(1) };
    assert_eq!(unsafe { CStr::from_ptr(second) }.to_bytes(), b"bob");
    assert!(!raw.transport.is_null());
    assert!(!unsafe { (*raw.transport).ctx }.is_null());
}
