//! Outbound-only shared client. Hosts own terminal state and native capabilities.
#![cfg_attr(
    not(feature = "local-host"),
    doc = r#"
Default mobile builds do not expose local execution contracts or transports:

```compile_fail
use shellfox_client::LocalClient;
```

```compile_fail
use shellfox_client::protocol::LocalRequest;
```

```compile_fail
use shellfox_client::protocol::automation::AutomationStart;
```

```compile_fail
use shellfox_client::protocol::DESKTOP_LAUNCH_NAMESPACE;
```
"#
)]
mod client;
mod commands;
mod state;
pub use state::{ClientError, ClientState, ConnectionStatus};
mod cursor;
mod transfers;
mod uploads;
pub use transfers::*;
pub use uploads::*;
#[cfg(feature = "local-host")]
mod local;
#[cfg(feature = "local-host")]
pub use local::*;

pub use client::*;
pub use cursor::*;
pub use shellfox_protocol as protocol;
