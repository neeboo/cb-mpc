use cb_mpc_sys::{BorrowedCmem, BorrowedCmems, OutCmem, OwnedCmem};

#[test]
fn cmem_keeps_its_input_buffer_alive_for_the_call() {
    let bytes = vec![1_u8, 2, 3, 4];
    let mem = BorrowedCmem::new(&bytes).expect("valid cmem");

    assert_eq!(mem.as_slice(), bytes);
}

#[test]
fn cmems_owns_flat_data_and_sizes_until_it_is_dropped() {
    let messages = vec![vec![1_u8, 2], vec![], vec![3, 4, 5]];
    let mems = BorrowedCmems::new(&messages).expect("valid cmems");

    assert_eq!(mems.to_vecs(), messages);
}

#[test]
fn out_pointer_is_a_real_mutable_slot() {
    let mut out = OutCmem::new();
    let first = out.as_mut_ptr();
    let second = out.as_mut_ptr();

    assert!(!first.is_null());
    assert_eq!(first, second);
}

#[test]
fn dropping_an_empty_c_output_is_null_safe() {
    drop(OwnedCmem::empty());
}
