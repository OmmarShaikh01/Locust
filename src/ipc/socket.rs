pub use interprocess::local_socket::traits::{Listener, Stream};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName};
use std::io;
use std::path::Path;

/// Generate a unique local socket / named pipe name based on project directory.
pub fn pipe_name_for_project(project_root: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let canonical = std::fs::canonicalize(project_root)
        .unwrap_or_else(|_| project_root.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();
    format!("locust-pipe-{:x}", hash)
}

pub fn create_listener(name: &str) -> io::Result<interprocess::local_socket::Listener> {
    let ns_name = name
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    ListenerOptions::new().name(ns_name).create_sync()
}

pub fn connect_stream(name: &str) -> io::Result<interprocess::local_socket::Stream> {
    let ns_name = name
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    interprocess::local_socket::Stream::connect(ns_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::framing::{read_frame, write_frame};
    use crate::ipc::protocol::{ClientCommand, ServerEvent};
    use std::thread;

    #[test]
    fn test_ipc_socket_roundtrip() {
        let pipe_name = format!("locust-test-pipe-{}", std::process::id());
        let listener = match create_listener(&pipe_name) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Skipping socket test: {e}");
                return;
            }
        };

        let server_thread = thread::spawn(move || {
            let mut conn = listener.accept().unwrap();
            let frame = read_frame(&mut conn).unwrap();
            let cmd: ClientCommand = serde_json::from_slice(&frame).unwrap();
            assert!(matches!(cmd, ClientCommand::Hello { .. }));

            let resp = ServerEvent::Alert {
                title: "OK".into(),
                message: "Connected".into(),
            };
            let serialized = serde_json::to_vec(&resp).unwrap();
            write_frame(&mut conn, &serialized).unwrap();
        });

        thread::sleep(std::time::Duration::from_millis(50));
        let mut client = connect_stream(&pipe_name).unwrap();
        let cmd = ClientCommand::Hello {
            client_version: "0.1.0".into(),
        };
        let serialized = serde_json::to_vec(&cmd).unwrap();
        write_frame(&mut client, &serialized).unwrap();

        let resp_frame = read_frame(&mut client).unwrap();
        let event: ServerEvent = serde_json::from_slice(&resp_frame).unwrap();
        assert!(matches!(event, ServerEvent::Alert { .. }));

        server_thread.join().unwrap();
    }
}
