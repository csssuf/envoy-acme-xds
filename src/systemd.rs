use tokio::net::UnixListener;

use crate::error::Result;

pub struct SystemdSocket {
    pub name: Option<String>,
    pub listener: UnixListener,
}

#[cfg(unix)]
pub fn systemd_listeners() -> Result<Vec<SystemdSocket>> {
    let fds = sd_listen_fds::get()?;
    let mut listeners = Vec::new();

    for (name, fd) in fds {
        let std_listener: std::os::unix::net::UnixListener = fd.into();
        std_listener.set_nonblocking(true)?;
        let listener = UnixListener::from_std(std_listener)?;
        listeners.push(SystemdSocket { name, listener });
    }

    Ok(listeners)
}

#[cfg(not(unix))]
pub fn systemd_listeners() -> Result<Vec<SystemdSocket>> {
    Ok(Vec::new())
}
