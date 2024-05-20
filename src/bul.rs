use std::{collections::VecDeque, sync::Arc};

use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use tokio::{
    sync::{mpsc, RwLock},
    task::JoinHandle,
    time::{self, Duration},
};
use tokio_util::sync::CancellationToken;

use promkit::{
    crossterm::{self, event, style::Color},
    grapheme::StyledGraphemes,
    style::StyleBuilder,
    switch::ActiveKeySwitcher,
    text_editor, PaneFactory,
};

mod keymap;
use crate::{
    container::{ContainerLog, ContainerLogStreamer, ContainerStateMatcher},
    terminal::Terminal,
    Signal,
};

/// Run the main application logic.
///
/// Set up and manages the text editor, terminal, and log streaming for container logs.
/// It handles user input and updates the display accordingly. The function continues to run until
/// a specific signal (`Signal::GoToDig` or `Signal::GoToBul`) is received, indicating a transition
/// to another part of the application.
///
/// # Arguments
/// * `text_editor` - State of the text editor used within the terminal.
/// * `api_pod` - Kubernetes API client configured for Pod resources.
/// * `pod_query` - Optional query string to filter pods.
/// * `container_state_matcher` - Matcher to filter containers based on their state.
/// * `pod_log_stream_timeout_duration` - Duration to wait before timing out the log stream.
/// * `render_interval_duration` - Interval at which the log stream is rendered.
/// * `queue_capacity` - Maximum number of log entries to store in memory.
///
/// # Returns
/// Returns a tuple containing the exit signal and a deque of `ContainerLog` entries if successful.
///
/// # Errors
/// This function can return an error if there are issues creating the terminal, reading from the event stream,
/// or interacting with the Kubernetes API.
pub async fn run(
    text_editor: text_editor::State,
    api_pod: Api<Pod>,
    pod_query: Option<String>,
    container_state_matcher: ContainerStateMatcher,
    log_retrieval_timeout: Duration,
    render_interval: Duration,
    queue_capacity: usize,
) -> anyhow::Result<(Signal, VecDeque<ContainerLog>)> {
    let keymap = ActiveKeySwitcher::new("default", keymap::default);
    let size = crossterm::terminal::size()?;

    let pane = text_editor.create_pane(size.0, size.1);
    let mut term = Terminal::new(&pane)?;
    term.draw_pane(&pane)?;

    let shared_term = Arc::new(RwLock::new(term));
    let shared_text_editor = Arc::new(RwLock::new(text_editor));
    let readonly_term = Arc::clone(&shared_term);
    let readonly_text_editor = Arc::clone(&shared_text_editor);

    let (log_stream_tx, mut log_stream_rx) = mpsc::channel(1);
    let container_log_streamer =
        ContainerLogStreamer::try_new(api_pod, pod_query, container_state_matcher)?;
    let canceler = CancellationToken::new();

    let canceled = canceler.clone();
    let log_streaming = tokio::spawn(async move {
        container_log_streamer
            .launch_log_streams(log_stream_tx, log_retrieval_timeout, canceled)
            .await?
            .collect::<Vec<_>>()
            .await;
        Ok(())
    });

    let log_keeping: JoinHandle<anyhow::Result<VecDeque<ContainerLog>>> =
        tokio::spawn(async move {
            let mut queue = VecDeque::with_capacity(queue_capacity);
            let interval = time::interval(render_interval);
            futures::pin_mut!(interval);

            loop {
                interval.tick().await;
                let maybe_log = log_stream_rx.recv().await;
                match maybe_log {
                    Some(log) => {
                        let text_editor = readonly_text_editor.read().await;
                        let size = crossterm::terminal::size()?;

                        if queue.len() > queue_capacity {
                            queue.pop_front().unwrap();
                        }
                        queue.push_back(log.clone());

                        if text_editor
                            .texteditor
                            .text_without_cursor()
                            .to_string()
                            .is_empty()
                        {
                            let merge = StyledGraphemes::from_iter([
                                log.meta,
                                StyledGraphemes::from(" "),
                                log.body,
                            ])
                            .matrixify(size.0 as usize, size.1 as usize, 0)
                            .0;
                            let term = readonly_term.read().await;
                            term.draw_stream_and_pane(
                                merge,
                                &text_editor.create_pane(size.0, size.1),
                            )?;
                        } else if let Some(body) = log.body.highlight(
                            &text_editor.texteditor.text_without_cursor().to_string(),
                            StyleBuilder::new()
                                .bgc(Color::Yellow)
                                .fgc(Color::Black)
                                .build(),
                        ) {
                            let merge = StyledGraphemes::from_iter([
                                log.meta,
                                StyledGraphemes::from(" "),
                                body,
                            ])
                            .matrixify(size.0 as usize, size.1 as usize, 0)
                            .0;
                            let term = readonly_term.read().await;
                            term.draw_stream_and_pane(
                                merge,
                                &text_editor.create_pane(size.0, size.1),
                            )?;
                        }
                    }
                    None => break,
                }
            }
            Ok(queue)
        });

    let mut signal: Signal;
    loop {
        let event = event::read()?;
        let mut text_editor = shared_text_editor.write().await;
        signal = keymap.get()(&event, &mut text_editor)?;
        if signal == Signal::GoToDig || signal == Signal::GoToBul {
            break;
        }

        let size = crossterm::terminal::size()?;
        let pane = text_editor.create_pane(size.0, size.1);
        let mut term = shared_term.write().await;
        term.draw_pane(&pane)?;
    }

    canceler.cancel();
    let _: anyhow::Result<(), anyhow::Error> = log_streaming.await?;

    Ok((signal, log_keeping.await??))
}
