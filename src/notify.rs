use std::collections::HashMap;
use std::time::Duration;
use tracing::debug;
use zbus::{Connection, Variant, proxy};

/// The notification server comes up with plasmashell, which can start after the daemon.
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(60);

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the D-Bus method's signature"
    )]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Shows a desktop notification in the background; failures are only logged.
pub fn spawn(connection: Connection, summary: String, body: String) {
    tokio::spawn(async move {
        let sent = tokio::time::timeout(NOTIFY_TIMEOUT, async {
            NotificationsProxy::new(&connection)
                .await?
                .notify(
                    "plasma-drop",
                    0,
                    "plasma-drop",
                    &summary,
                    &body,
                    &[],
                    &HashMap::new(),
                    -1,
                )
                .await
        })
        .await;
        match sent {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => debug!("failed to show notification: {error}"),
            Err(_) => debug!("timed out showing notification"),
        }
    });
}
