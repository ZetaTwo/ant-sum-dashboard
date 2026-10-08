#![cfg(windows)]

use std::ffi::OsString;
use std::sync::OnceLock;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use crate::cli::Cli;

const SERVICE_NAME: &str = "AntSumDashboard";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

// `ffi_service_main`'s signature is fixed by the macro and can't take extra
// arguments, so the parsed CLI is stashed here for `service_main_entry` to
// pick up once the SCM dispatches to it.
static CLI: OnceLock<Cli> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main_entry);

/// Attempts to register this process as a Windows service and run as one.
///
/// `service_dispatcher::start` is itself the detector for "was this process
/// launched by the Service Control Manager?": if so, it blocks this thread
/// until the service stops and returns `Ok(())`; if the process was started
/// any other way (console, debugger, double-click), there is no SCM pipe to
/// attach to and it fails immediately with `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`.
/// The caller should treat `Err` as "not running as a service" and fall
/// through to normal interactive startup, not as a fatal error.
pub fn try_run_as_service(cli: Cli) -> windows_service::Result<()> {
    CLI.set(cli)
        .expect("try_run_as_service must only be called once");
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn service_main_entry(_arguments: Vec<OsString>) {
    // No console in service context and logging isn't initialized yet at
    // this point, so there's nowhere to report an error to; just exit.
    let _ = run_service();
}

fn run_service() -> anyhow::Result<()> {
    let cli = CLI.get().expect("CLI not set before dispatch").clone();

    let log_dir = cli.log_dir.clone().unwrap_or_else(default_log_dir);
    std::fs::create_dir_all(&log_dir)?;
    let _guard = crate::logging::init_service(&log_dir);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let status_handle = service_control_handler::register(SERVICE_NAME, move |control| {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(true);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;

    let set_status = |state: ServiceState, controls_accepted: ServiceControlAccept| {
        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: state,
            controls_accepted,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
    };

    set_status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    )?;

    tracing::info!(log_dir = %log_dir.display(), "starting as a Windows service");

    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(crate::app::run(cli, async move {
        let _ = shutdown_rx.wait_for(|stopped| *stopped).await;
    }));

    // Bounded rather than an implicit `drop(rt)`: the ANT+ USB polling loop
    // only notices the dropped update channel on its next synchronous poll
    // iteration (bounded by the ANT+ broadcast interval), and there's no
    // persisted state to lose by not waiting for it. Capping this here
    // means we reliably report Stopped quickly instead of risking the SCM
    // deciding we didn't respond in time and force-killing the process.
    rt.shutdown_timeout(Duration::from_secs(5));

    set_status(ServiceState::Stopped, ServiceControlAccept::empty())?;
    result
}

fn default_log_dir() -> std::path::PathBuf {
    std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\ProgramData"))
        .join("ant-sum-dashboard")
        .join("logs")
}
