//! Bounded, account-isolated view of messages deleted while they were loaded.
//! Archived rows are display-only and never become reply, edit or reaction targets.
use crate::{design, i18n::translate, local_time};
use client_core::State;
use model::Id;

/// One archived row. Mirrors `local_store::DeletedMessage` without making the UI depend on the
/// storage crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
	pub channel: Id,
	pub id: Id,
	pub author_name: String,
	pub content: String,
	pub sent_at: i64,
	pub deleted_at: i64,
	pub attachments: Vec<model::Attachment>,
}

const PAGE: usize = 100;

#[derive(Default)]
pub struct DeletedMessagesUi {
	account: Option<Id>,
	request: u64,
	pending: Option<(u64, usize)>,
	loading: bool,
	loaded: bool,
	pub entries: Vec<Entry>,
	pub has_more: bool,
	pub error: Option<&'static str>,
	/// The owner asked to purge the archive; the host queues the store operation.
	pub clear_requested: bool,
}

impl DeletedMessagesUi {
	/// Starts the first page once for the current account; a switched account starts over.
	pub fn ensure_loaded(&mut self, account: Id) {
		if self.account != Some(account) {
			self.account = Some(account);
			self.entries.clear();
			self.pending = None;
			self.loading = false;
			self.loaded = false;
			self.has_more = false;
			self.error = None;
		}
		if !self.loaded && !self.loading && self.pending.is_none() {
			self.request_page(0);
		}
	}
	/// The host takes at most one request per frame and reports the outcome with `loaded`.
	pub fn take_request(&mut self) -> Option<(u64, usize)> {
		self.pending.take()
	}
	pub fn loaded(&mut self, request: u64, offset: usize, result: Result<Vec<Entry>, ()>) {
		if request != self.request {
			return;
		}
		self.loading = false;
		self.loaded = true;
		match result {
			Ok(mut entries) => {
				self.has_more = entries.len() == PAGE;
				if offset == 0 {
					self.entries = entries;
				} else {
					self.entries.append(&mut entries);
				}
			}
			Err(()) => {
				self.has_more = false;
				self.error = Some("Could not read the archived deleted messages.");
			}
		}
	}
	/// The storage worker stopped before answering; keep any rows already shown.
	pub fn load_failed(&mut self) {
		self.loading = false;
		self.has_more = false;
		self.error =
			Some("Local storage worker stopped; restart Serein to read archived messages.");
	}
	pub fn clear_finished(&mut self, ok: bool) {
		self.loading = false;
		if ok {
			self.entries.clear();
			self.loaded = false;
			self.has_more = false;
			self.error = None;
		} else {
			self.error = Some("Could not clear the archived deleted messages.");
		}
	}
	fn request_page(&mut self, offset: usize) {
		self.request = self.request.wrapping_add(1);
		self.loading = true;
		self.error = None;
		self.pending = Some((self.request, offset));
	}
}

pub(crate) fn page(
	ui: &mut egui::Ui,
	state: &State,
	view: &mut DeletedMessagesUi,
	enabled: &mut bool,
) {
	design::group(
		ui,
		&translate("settings-deleted-messages-page-archive"),
		|ui| {
			ui.add_enabled_ui(!state.demo, |ui| {
				let response = design::switch(
					ui,
					"settings-deleted-messages-page-archive",
					Some("settings-deleted-messages-page-archive-description"),
					enabled,
				);
				if response.changed() && !*enabled {
					// Disabling the opt-in removes what this device already kept.
					view.entries.clear();
					view.loaded = false;
					view.clear_requested = true;
				}
			});
			design::hint(
				ui,
				&translate("settings-deleted-messages-page-archive-bounds"),
			);
		},
	);
	if !*enabled {
		return;
	}
	design::group(
		ui,
		&translate("settings-deleted-messages-page-archived-messages"),
		|ui| {
			if let Some(error) = view.error {
				design::hint(ui, error);
				if design::text_action(ui, &translate("settings-deleted-messages-page-retry"))
					.clicked()
				{
					view.error = None;
					view.loaded = false;
				}
			}
			if view.loading && view.entries.is_empty() {
				design::hint(ui, &translate("settings-deleted-messages-page-loading"));
				return;
			}
			if view.entries.is_empty() {
				design::hint(ui, &translate("settings-deleted-messages-page-empty"));
			}
			let mut load_more = false;
			for entry in &view.entries {
				design::card_divider(ui);
				let channel = state
					.channel(entry.channel)
					.map(|channel| channel.name.clone())
					.unwrap_or_else(|| entry.channel.to_string());
				ui.horizontal_wrapped(|ui| {
					ui.spacing_mut().item_spacing.x = 6.0;
					ui.label(design::semibold(ui, &entry.author_name, 14.0));
					ui.label(
						egui::RichText::new(format!("#{channel}"))
							.size(12.0)
							.color(design::palette(ui).muted),
					);
				});
				ui.add(egui::Label::new(egui::RichText::new(&entry.content).size(14.0)).wrap());
				for attachment in &entry.attachments {
					ui.add(
						egui::Label::new(
							egui::RichText::new(&attachment.filename)
								.size(12.0)
								.color(design::palette(ui).muted),
						)
						.truncate(),
					);
				}
				ui.label(
					egui::RichText::new(format!(
						"{} {}  ·  {} {}",
						translate("settings-deleted-messages-page-sent"),
						time_label(entry.sent_at),
						translate("settings-deleted-messages-page-deleted"),
						time_label(entry.deleted_at)
					))
					.size(12.0)
					.color(design::palette(ui).muted),
				);
			}
			if view.has_more {
				design::card_divider(ui);
				if design::button(
					ui,
					&translate("settings-deleted-messages-page-load-more"),
					design::ButtonKind::Outline,
				)
				.clicked()
				{
					load_more = true;
				}
			}
			if load_more {
				view.request_page(view.entries.len());
			}
			if !view.entries.is_empty() {
				design::card_divider(ui);
				if design::text_action(
					ui,
					&translate("settings-deleted-messages-page-clear-archive"),
				)
				.clicked()
				{
					view.clear_requested = true;
				}
			}
		},
	);
}

fn time_label(seconds: i64) -> String {
	time::OffsetDateTime::from_unix_timestamp(seconds)
		.map(local_time::full)
		.unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(id: u64) -> Entry {
		Entry {
			channel: Id(2),
			id: Id(id),
			author_name: "Synthetic".into(),
			content: "body".into(),
			sent_at: 1,
			deleted_at: 2,
			attachments: Vec::new(),
		}
	}

	#[test]
	fn pages_append_and_stale_responses_are_ignored() {
		let mut view = DeletedMessagesUi::default();
		view.ensure_loaded(Id(1));
		let (first_request, offset) = view.take_request().unwrap();
		assert_eq!(offset, 0);
		view.loaded(first_request, 0, Ok((0..100).map(entry).collect()));
		assert!(view.has_more);
		assert_eq!(view.entries.len(), 100);

		view.request_page(100);
		let (second_request, offset) = view.take_request().unwrap();
		assert_eq!(offset, 100);
		// A stale completion from the earlier request must not replace the page.
		view.loaded(first_request, 0, Ok(vec![entry(1)]));
		assert_eq!(view.entries.len(), 100);
		view.loaded(second_request, 100, Ok(vec![entry(101)]));
		assert_eq!(view.entries.len(), 101);
		assert!(!view.has_more);

		// Two calls for the same opening leave one in-flight request.
		let mut view = DeletedMessagesUi::default();
		view.ensure_loaded(Id(1));
		view.ensure_loaded(Id(1));
		assert!(view.take_request().is_some());
		assert!(view.take_request().is_none());

		// A different account starts over instead of showing the previous account's rows.
		view.loaded(1, 0, Ok(vec![entry(1)]));
		view.ensure_loaded(Id(9));
		assert!(view.entries.is_empty());
		assert!(view.take_request().is_some());
	}
}
