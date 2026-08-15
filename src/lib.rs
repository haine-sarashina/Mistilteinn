pub mod app;
pub mod browser;
pub mod css;
pub mod html;
pub mod js;
pub mod layout;
#[cfg(feature = "memprof")]
pub mod memprof;
pub mod network;
pub mod page;
pub mod render;

pub mod update;

#[cfg(test)]
pub mod tests {
    #[test]
    fn engine_smoke_test() {
        assert!(true, "Mistilteinn engine test suite initialized");
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Mistilteinn starting...");

    // 起動時に別スレッドでアップデートチェックを開始
    std::thread::spawn(update::check_and_update);

    let start_url = std::env::var("MISTILTEIN_URL").ok();
    app::run(start_url);
}
