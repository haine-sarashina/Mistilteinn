fn main() {
    let handle = {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.handle().clone()
    };
    handle.block_on(async {
        println!("Hello world");
    });
}
