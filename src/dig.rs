use std::collections::VecDeque;

use rayon::prelude::*;

use promkit::{
    crossterm::{event::Event, style::Color},
    grapheme::StyledGraphemes,
    listbox,
    pane::Pane,
    snapshot::Snapshot,
    style::StyleBuilder,
    switch::ActiveKeySwitcher,
    text_editor, PaneFactory, Prompt, PromptSignal,
};

use crate::container::ContainerLog;

mod keymap;

pub struct Digger {
    keymap: ActiveKeySwitcher<keymap::Keymap>,
    text_editor_snapshot: Snapshot<text_editor::State>,
    log_queue: VecDeque<ContainerLog>,
    logs_snapshot: Snapshot<listbox::State>,
}

impl promkit::Finalizer for Digger {
    type Return = ();

    fn finalize(&self) -> anyhow::Result<Self::Return> {
        Ok(())
    }
}

impl promkit::Renderer for Digger {
    fn create_panes(&self, width: u16, height: u16) -> Vec<Pane> {
        vec![
            self.logs_snapshot.create_pane(width, height),
            self.text_editor_snapshot.create_pane(width, height),
        ]
    }

    fn evaluate(&mut self, event: &Event) -> anyhow::Result<PromptSignal> {
        let signal = self.keymap.get()(
            event,
            &mut self.text_editor_snapshot,
            &mut self.logs_snapshot,
        );
        if self
            .text_editor_snapshot
            .after()
            .texteditor
            .text_without_cursor()
            != self
                .text_editor_snapshot
                .borrow_before()
                .texteditor
                .text_without_cursor()
        {
            let query = self
                .text_editor_snapshot
                .after()
                .texteditor
                .text_without_cursor()
                .to_string();

            let list: Vec<StyledGraphemes> = self
                .log_queue
                .par_iter()
                .filter_map(|log| {
                    log.body
                        .clone()
                        .highlight(
                            &query,
                            StyleBuilder::new()
                                .bgc(Color::Yellow)
                                .fgc(Color::Black)
                                .build(),
                        )
                        .map(|body| {
                            StyledGraphemes::from_iter([
                                &log.meta,
                                &StyledGraphemes::from(" "),
                                &body,
                            ])
                        })
                })
                .collect();

            self.logs_snapshot.after_mut().listbox = listbox::Listbox::from_iter(list);
        }
        signal
    }
}

pub fn run(
    text_editor: text_editor::State,
    log_queue: VecDeque<ContainerLog>,
    mut logs: listbox::State,
) -> anyhow::Result<()> {
    logs.listbox = listbox::Listbox::from_iter(
        log_queue
            .par_iter()
            .map(|log| {
                StyledGraphemes::from_iter([&log.meta, &StyledGraphemes::from(" "), &log.body])
            })
            .collect::<Vec<StyledGraphemes>>(),
    );
    Prompt {
        renderer: Digger {
            keymap: ActiveKeySwitcher::new("default", keymap::default),
            text_editor_snapshot: Snapshot::new(text_editor),
            log_queue,
            logs_snapshot: Snapshot::new(logs),
        },
    }
    .run()
}
