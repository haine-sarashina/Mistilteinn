use log::{error, info};
use self_update::cargo_crate_version;

/// Checks for updates in the background and applies them if available.
/// This should be run on a separate thread to avoid blocking the main UI.
pub fn check_and_update() {
    info!("Checking for updates...");

    // We run the update logic inside a closure to cleanly handle errors via `?`
    let result = (|| -> Result<self_update::Status, Box<dyn std::error::Error>> {
        let status = self_update::backends::github::Update::configure()
            .repo_owner("haine-sarashina")
            .repo_name("Mistilteinn")
            .bin_name("mistilteinn")
            .show_download_progress(true)
            .no_confirm(true)
            .current_version(cargo_crate_version!())
            .build()?
            .update()?;

        Ok(status)
    })();

    match result {
        Ok(status) => {
            if status.updated() {
                info!("Successfully updated to {}", status.version());
                let ans = rfd::MessageDialog::new()
                    .set_title("Mistilteinn Update")
                    .set_description(&format!("新しいバージョン ({}) へのアップデートが完了しました。\n今すぐ再起動しますか？", status.version()))
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();

                if ans == rfd::MessageDialogResult::Yes {
                    if let Ok(current_exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(current_exe).spawn();
                        std::process::exit(0);
                    }
                }
            } else {
                info!("App is already up-to-date (version {}).", status.version());
            }
        }
        Err(e) => {
            error!("Failed to check/apply updates: {}", e);
        }
    }
}
