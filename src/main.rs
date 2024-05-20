use std::io;

use clap::Parser;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    config::{KubeConfigOptions, Kubeconfig},
    Api, Client, Config,
};
use tokio::time::Duration;

use promkit::{
    crossterm::{
        self, cursor, execute,
        style::Color,
        terminal::{disable_raw_mode, enable_raw_mode},
    },
    listbox,
    style::StyleBuilder,
    text_editor,
};

mod bul;
mod container;
use container::{ContainerState, ContainerStateMatcher};
mod dig;
mod terminal;

#[derive(PartialEq, Eq)]
pub enum Signal {
    Continue,
    GoToDig,
    GoToBul,
}

/// Interactive Kubernetes log viewer
#[derive(Parser)]
#[command(name = "bul", version)]
pub struct Args {
    #[arg(long = "context", help = "Kubernetes context.")]
    pub context: Option<String>,

    #[arg(short = 'n', long = "namespace", help = "Kubernetes namespace.")]
    pub namespace: Option<String>,

    #[arg(short = 'p', long = "pod-query", help = "query to filter Pods.")]
    pub pod_query: Option<String>,

    #[arg(
        long = "container-states",
        help = "Container states to filter containers.",
        value_delimiter = ',',
        default_value = "all"
    )]
    pub container_status: Vec<ContainerState>,

    #[arg(
        long = "log-retrieval-timeout",
        default_value = "10",
        help = "Timeout to read a next line from the log stream in milliseconds."
    )]
    pub log_retrieval_timeout_millis: u64,

    #[arg(
        long = "render-interval",
        default_value = "10",
        help = "Interval to render a log line in milliseconds.",
        long_help = "Adjust this value to prevent screen flickering
        when a large volume of logs is rendered in a short period."
    )]
    pub render_interval_millis: u64,

    #[arg(
        short = 'q',
        long = "queue-capacity",
        default_value = "1000",
        help = "Queue capacity to store the logs.",
        long_help = "Queue capacity for storing logs.
        This value is used for temporary storage of log data
        and should be adjusted based on the system's memory capacity.
        Increasing this value allows for more logs to be stored temporarily,
        which can be beneficial when digging deeper into logs with the digger."
    )]
    pub queue_capacity: usize,
}

/// Detects the Kubernetes context based on the provided `Args`.
///
/// Context determination follows this priority:
/// 1. Uses the context explicitly specified in the `Args` structure.
/// 2. Retrieves the current context from the kubeconfig file.
///
/// # Errors
/// Returns an error if the kubeconfig file cannot be read or if no current context is set in the kubeconfig.
fn detect_context(args: &Args) -> anyhow::Result<String> {
    match &args.context {
        Some(context) => Ok(context.clone()),
        None => {
            let kubeconfig = Kubeconfig::read()?;
            Ok(kubeconfig
                .current_context
                .ok_or_else(|| anyhow::anyhow!("current_context is not set"))?)
        }
    }
}

/// Detects the Kubernetes namespace based on the provided `Args`.
///
/// Namespace determination follows this priority:
/// 1. Uses the namespace explicitly specified in the `Args` structure.
/// 2. Retrieves the default namespace associated with the current context from kubeconfig.
/// 3. Uses "default".
fn detect_namespace(args: &Args, context: &str) -> anyhow::Result<String> {
    let kubeconfig = Kubeconfig::read()?;
    let default_namespace = kubeconfig
        .contexts
        .iter()
        .find(|c| Some(c.name.as_str()) == Some(context))
        .and_then(|context| {
            context
                .context
                .as_ref()
                .and_then(|ctx| ctx.namespace.clone())
        })
        .unwrap_or_else(|| String::from("default"));
    Ok(args.namespace.clone().unwrap_or(default_namespace))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let context = detect_context(&args)?;
    let namespace = detect_namespace(&args, &context)?;

    let kubeconfig = Kubeconfig::read()?;
    let options = KubeConfigOptions {
        context: Some(context),
        ..Default::default()
    };
    let config = Config::from_custom_kubeconfig(kubeconfig, &options).await?;
    let api_pod: Api<Pod> = Api::namespaced(Client::try_from(config)?, &namespace);

    enable_raw_mode()?;
    execute!(io::stdout(), cursor::Hide)?;

    while let Ok((signal, queue)) = bul::run(
        text_editor::State {
            texteditor: Default::default(),
            history: Default::default(),
            prefix: String::from("❯❯ "),
            mask: Default::default(),
            prefix_style: StyleBuilder::new().fgc(Color::DarkGreen).build(),
            active_char_style: StyleBuilder::new().bgc(Color::DarkCyan).build(),
            inactive_char_style: StyleBuilder::new().build(),
            edit_mode: Default::default(),
            word_break_chars: Default::default(),
            lines: Default::default(),
        },
        api_pod.clone(),
        args.pod_query.clone(),
        ContainerStateMatcher::new(args.container_status.clone()),
        Duration::from_millis(args.log_retrieval_timeout_millis),
        Duration::from_millis(args.render_interval_millis),
        args.queue_capacity,
    )
    .await
    {
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::Purge),
            cursor::MoveTo(0, 0),
        )?;

        match signal {
            Signal::GoToDig => {
                dig::run(
                    text_editor::State {
                        texteditor: Default::default(),
                        history: Default::default(),
                        prefix: String::from("❯❯❯ "),
                        mask: Default::default(),
                        prefix_style: StyleBuilder::new().fgc(Color::DarkBlue).build(),
                        active_char_style: StyleBuilder::new().bgc(Color::DarkCyan).build(),
                        inactive_char_style: StyleBuilder::new().build(),
                        edit_mode: Default::default(),
                        word_break_chars: Default::default(),
                        lines: Default::default(),
                    },
                    queue,
                    listbox::State {
                        listbox: listbox::Listbox::default(),
                        cursor: String::from("❯ "),
                        active_item_style: None,
                        inactive_item_style: None,
                        lines: Default::default(),
                    },
                )?;

                // Re-enable raw mode and hide the cursor again here
                // because they are disabled and shown, respectively, by promkit.
                enable_raw_mode()?;
                execute!(io::stdout(), cursor::Hide)?;

                crossterm::execute!(
                    io::stdout(),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::Purge),
                    cursor::MoveTo(0, 0),
                )?;
            }
            Signal::GoToBul => {
                continue;
            }
            _ => {}
        }
    }

    execute!(io::stdout(), cursor::Show)?;
    disable_raw_mode()?;

    Ok(())
}
