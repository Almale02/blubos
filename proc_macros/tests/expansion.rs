// The magic: bring in the crate as if you were an external user.
use proc_macros::bitstruct; // whatever your crate is named

#[test]
fn basic_bitstruct_should_compile_and_run() {
    bitstruct! {
        struct BitStruct {
            pub flag1: bool = 0,
            pub flag2: bool,
            pub data1: u8 = 3..=10,
        }
    }
    let mut b = BitStruct::new();
    b.set_flag1(true);
    b.set_data1(0x55);
    assert!(b.flag1());
    assert_eq!(b.data1(), 0x55);
}
