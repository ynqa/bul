use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use futures::{stream::FuturesUnordered, AsyncBufReadExt, StreamExt};
use k8s_openapi::api::{self, core::v1::Pod};
use kube::api::{Api, ListParams, LogParams};
use regex::Regex;
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{timeout, Duration},
};
use tokio_util::sync::CancellationToken;

use promkit::{crossterm::style::Color, grapheme::StyledGraphemes, style::StyleBuilder};

#[derive(Clone)]
pub struct ContainerLog {
    pub meta: StyledGraphemes,
    pub body: StyledGraphemes,
}

#[derive(Clone, clap::ValueEnum, Debug, PartialEq)]
pub enum ContainerState {
    All,
    Running,
    Terminated,
    Waiting,
}

pub struct ContainerStateMatcher(Vec<ContainerState>);

impl ContainerStateMatcher {
    pub fn new(states: Vec<ContainerState>) -> Self {
        Self(states)
    }

    pub fn matches(&self, state: &api::core::v1::ContainerState) -> bool {
        if self.0.contains(&ContainerState::All) {
            true
        } else {
            self.0.iter().any(|accept| match accept {
                ContainerState::Running => state.running.is_some(),
                ContainerState::Terminated => state.terminated.is_some(),
                ContainerState::Waiting => state.waiting.is_some(),
                _ => false,
            })
        }
    }
}

pub struct ContainerLogStreamer {
    api_pod: Api<Pod>,
    pod_regex: Option<Regex>,
    container_state_matcher: ContainerStateMatcher,
    colors: Vec<Color>,
}

impl ContainerLogStreamer {
    pub fn try_new(
        api_pod: Api<Pod>,
        pod_query: Option<String>,
        container_state_matcher: ContainerStateMatcher,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            api_pod,
            pod_regex: match pod_query {
                Some(query) => Some(Regex::new(&query)?),
                None => None,
            },
            container_state_matcher,
            colors: vec![
                Color::Red,
                Color::DarkRed,
                Color::Green,
                Color::DarkGreen,
                Color::Yellow,
                Color::DarkYellow,
                Color::Blue,
                Color::DarkBlue,
                Color::Magenta,
                Color::DarkMagenta,
                Color::Cyan,
                Color::DarkCyan,
            ],
        })
    }

    /// Retrieves a vector of pairs of pod and container names
    /// that match specific criteria from a list of Pods obtained via the API.
    ///
    /// The function operates as follows:
    /// 1. Initializes an empty vector `ret`.
    /// 2. Uses `api_pod.list` to fetch a list of Pods with default list parameters.
    /// 3. For each Pod retrieved, it performs the following checks:
    ///    - Whether the Pod's name matches the regular expression `pod_regex`, if it is set.
    ///    - Whether the Pod's status exists and if any of the container statuses
    ///      match specific states defined by `container_state_matcher`.
    /// 4. For each container that matches the conditions, adds a pair of the Pod's name and the container's name to the vector `ret`.
    /// 5. After checking all Pods and their containers, returns the vector `ret`.
    async fn get_pod_and_containers(&self) -> anyhow::Result<Vec<(String, String)>> {
        let mut ret = Vec::new();

        for pod in self.api_pod.list(&ListParams::default()).await? {
            if let Some(pod_name) = pod.metadata.name {
                if let Some(pod_regex) = &self.pod_regex {
                    if !pod_regex.is_match(&pod_name) {
                        continue;
                    }
                }
                if let Some(pod_status) = pod.status {
                    if let Some(container_statuses) = pod_status.container_statuses {
                        for container in container_statuses.iter().filter(|status| {
                            status
                                .state
                                .as_ref()
                                .map_or(false, |state| self.container_state_matcher.matches(state))
                        }) {
                            ret.push((pod_name.clone(), container.name.clone()));
                        }
                    }
                }
                if let Some(containers) = pod.spec.map(|spec| spec.containers) {
                    for container in containers {
                        ret.push((pod_name.clone(), container.name));
                    }
                }
            }
        }

        Ok(ret)
    }

    /// Initiates log streams for pods and containers that match specified criteria.
    pub async fn launch_log_streams(
        &self,
        log_stream_tx: mpsc::Sender<ContainerLog>,
        log_retrieval_timeout: Duration,
        canceled: CancellationToken,
    ) -> anyhow::Result<FuturesUnordered<JoinHandle<Result<(), anyhow::Error>>>> {
        let futures = FuturesUnordered::new();
        let pod_containers = self.get_pod_and_containers().await?;

        for (pod, container) in pod_containers.iter() {
            // If cancellation is detected (e.g. pressing ctrl+c immediately after execution),
            // break early to avoid creating unnecessary futures.
            if canceled.is_cancelled() {
                break;
            }

            let log_stream_tx = log_stream_tx.clone();
            let colors = self.colors.clone();

            let mut pod_log_stream = self
                .api_pod
                .log_stream(
                    pod,
                    &LogParams {
                        container: Some(container.clone()),
                        follow: true,
                        ..Default::default()
                    },
                )
                .await?
                .lines();

            let mut hasher = DefaultHasher::new();
            let key = format!("{} {}", &pod, &container);
            key.hash(&mut hasher);
            let hashed = hasher.finish();
            let canceled = canceled.clone();
            let color = colors[hashed as usize % colors.len()];

            futures.push(tokio::spawn(async move {
                while !canceled.is_cancelled() {
                    // Set a timeout to ensure non-blocking behavior,
                    // especially responsive to user inputs like ctrl+c.
                    // Continuously retry until cancellation to prevent loss of logs.
                    let ret = timeout(log_retrieval_timeout, pod_log_stream.next()).await;
                    if ret.is_err() {
                        continue;
                    }

                    let ret = ret?;

                    match ret {
                        Some(Ok(line)) => {
                            let escaped =
                                strip_ansi_escapes::strip_str(line.replace(['\n', '\t'], " "));
                            log_stream_tx
                                .send(ContainerLog {
                                    meta: StyledGraphemes::from_str(
                                        &key,
                                        StyleBuilder::new().fgc(color).build(),
                                    ),
                                    body: StyledGraphemes::from_str(
                                        &escaped,
                                        StyleBuilder::new().fgc(Color::Reset).build(),
                                    ),
                                })
                                .await?;
                        }
                        _ => break,
                    }
                }
                Ok(())
            }));
        }

        Ok(futures)
    }
}
