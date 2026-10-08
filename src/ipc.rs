//! Single-instance support. A second launch (e.g. `snapcap --shot region` bound to a
//! desktop shortcut) forwards its action to the running instance and exits.

use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Name, Stream};

use crate::actions::Action;
use crate::error::Result;

fn name() -> std::io::Result<Name<'static>> {
    "dev.snapcap.ipc.sock".to_ns_name::<GenericNamespaced>()
}

/// Returns true if another instance received the action.
pub fn send_to_running(action: Action) -> bool {
    let Ok(name) = name() else { return false };
    let Ok(mut conn) = Stream::connect(name) else { return false };
    conn.write_all(format!("{}\n", action.cli_name()).as_bytes()).is_ok()
}

/// Accepts actions from later launches on a background thread.
pub fn listen(on_action: impl Fn(Action) + Send + 'static) -> Result<()> {
    let listener = ListenerOptions::new().name(name()?).try_overwrite(true).create_sync()?;
    std::thread::Builder::new().name("snapcap-ipc".into()).spawn(move || {
        for conn in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(conn).read_line(&mut line).is_ok() {
                if let Some(a) = Action::from_cli_name(line.trim()) {
                    on_action(a);
                }
            }
        }
    })?;
    Ok(())
}
