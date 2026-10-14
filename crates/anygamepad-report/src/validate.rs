//! [`Report::validate`]: what a well-formed report must also satisfy.

use core::fmt;

use anygamepad_model::hid::HidCollection;
use anygamepad_model::identity::{DescriptorHash, DeviceIdentity, ProviderKind};
use anygamepad_model::raw::RawLayout;

use crate::privacy::{is_identifying_input_report, text_findings};
use crate::{
    App, FORMAT, HidData, MAX_DESCRIPTOR_BYTES, MAX_ELEMENTS, MAX_HID_ENTRIES, MAX_HID_NESTING,
    MAX_SAMPLE_BYTES, MAX_SAMPLES, MAX_TEXT_CHARS, MAX_USER_TEXT_CHARS, MAX_VIEWS, Report,
    SCHEMA_VERSION, View,
};

/// Error returned by [`Report::validate`]: the first problem found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidReport {
    message: String,
}

impl InvalidReport {
    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid device report: {}", self.message)
    }
}

impl std::error::Error for InvalidReport {}

impl Report {
    /// Checks what the types cannot:
    ///
    /// - the format, the schema version, and between one and
    ///   [`MAX_VIEWS`] views;
    /// - web reports name their browser and have web views only; desktop
    ///   reports have neither; every view is of the host's OS and browser;
    /// - fingerprints and descriptor hashes match what they hash;
    /// - no text holds what looks like personal data, control characters or
    ///   more than [`MAX_TEXT_CHARS`] characters
    ///   ([`MAX_USER_TEXT_CHARS`] for `user.product`), and no input sample
    ///   is of a report that can carry identifiers (see
    ///   [`privacy`](crate::privacy));
    /// - sizes stay within the limits of this crate, axis ranges are finite,
    ///   and confirmed mappings are valid and unconditional.
    ///
    /// Run it before storing, sharing or accepting a report; clients run
    /// [`scrub`](Self::scrub) first.
    pub fn validate(&self) -> Result<(), InvalidReport> {
        self.check().map_err(|message| InvalidReport { message })
    }

    fn check(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("`format` is `{}`, not `{FORMAT}`", self.format));
        }
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "`schema_version` is {}; this library reads {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let web = self.source.app == App::Web;
        match (web, self.host.browser.is_some()) {
            (true, false) => return Err("web reports need `host.browser`".into()),
            (false, true) => return Err("desktop reports have no `host.browser`".into()),
            _ => {}
        }
        if let Some(version) = &self.host.os_version {
            check_text("host.os_version", version, MAX_TEXT_CHARS)?;
        }
        if let Some(product) = &self.user.product {
            if product.trim().is_empty() {
                return Err("`user.product` is empty".into());
            }
            check_text("user.product", product, MAX_USER_TEXT_CHARS)?;
        }
        if self.views.is_empty() {
            return Err("no views".into());
        }
        if self.views.len() > MAX_VIEWS {
            return Err(format!("{} views; at most {MAX_VIEWS}", self.views.len()));
        }
        for (index, view) in self.views.iter().enumerate() {
            self.check_view(&format!("views[{index}]"), view)?;
        }
        Ok(())
    }

    fn check_view(&self, path: &str, view: &View) -> Result<(), String> {
        let identity = &view.identity;
        if identity.os != self.host.os {
            return Err(format!(
                "{path}: OS `{}` differs from the host's `{}`",
                identity.os, self.host.os
            ));
        }
        let web = self.source.app == App::Web;
        if (identity.provider == ProviderKind::Web) != web {
            return Err(format!(
                "{path}: a `{}` view in a {} report",
                identity.provider,
                if web { "web" } else { "desktop" }
            ));
        }
        if identity.browser != self.host.browser {
            return Err(format!("{path}: its browser differs from the host's"));
        }
        if view.fingerprint != identity.fingerprint() {
            return Err(format!(
                "{path}: `fingerprint` is {}, but the identity's is {}",
                view.fingerprint,
                identity.fingerprint()
            ));
        }
        for (name, text) in identity_texts(identity) {
            check_text(&format!("{path}.identity.{name}"), text, MAX_TEXT_CHARS)?;
        }
        if let Some(profile) = &view.profile {
            check_text(&format!("{path}.profile"), profile, MAX_TEXT_CHARS)?;
        }
        check_layout(path, &view.layout)?;
        if let Some(hid) = &view.hid {
            check_hid(path, identity, hid)?;
        }
        if let Some(mapping) = &view.confirmed {
            if !mapping.when.is_empty() {
                return Err(format!("{path}.confirmed: `when` must be empty"));
            }
            mapping
                .validate()
                .map_err(|error| format!("{path}.confirmed: {}", error.message()))?;
        }
        Ok(())
    }
}

fn check_text(path: &str, text: &str, max_chars: usize) -> Result<(), String> {
    match text_findings(text, max_chars).first() {
        Some(finding) => Err(format!("{path} has {finding}")),
        None => Ok(()),
    }
}

/// The text fields of an identity, by name.
fn identity_texts(identity: &DeviceIdentity) -> Vec<(&'static str, &str)> {
    let mut texts = Vec::new();
    for (name, text) in [
        ("vendor_name", &identity.vendor_name),
        ("product_name", &identity.product_name),
        ("driver", &identity.driver),
    ] {
        if let Some(text) = text {
            texts.push((name, text.as_str()));
        }
    }
    if let Some(web) = &identity.web {
        texts.push(("web.id", web.id.as_str()));
        texts.push(("web.mapping", web.mapping.as_str()));
    }
    texts
}

fn check_layout(path: &str, layout: &RawLayout) -> Result<(), String> {
    for (name, len) in [
        ("buttons", layout.buttons.len()),
        ("axes", layout.axes.len()),
        ("hats", layout.hats.len()),
        ("touchpads", layout.touchpads.len()),
    ] {
        if len > MAX_ELEMENTS {
            return Err(format!(
                "{path}.layout: {len} {name}; at most {MAX_ELEMENTS}"
            ));
        }
    }
    for (index, axis) in layout.axes.iter().enumerate() {
        if !axis.min.is_finite() || !axis.max.is_finite() {
            return Err(format!("{path}.layout.axes[{index}]: range is not finite"));
        }
    }
    Ok(())
}

fn check_hid(path: &str, identity: &DeviceIdentity, hid: &HidData) -> Result<(), String> {
    if let Some(descriptor) = &hid.descriptor {
        if descriptor.0.len() > MAX_DESCRIPTOR_BYTES {
            return Err(format!(
                "{path}.hid.descriptor: {} bytes; at most {MAX_DESCRIPTOR_BYTES}",
                descriptor.0.len()
            ));
        }
        let hash = DescriptorHash::of(&descriptor.0);
        if identity.descriptor_hash != Some(hash) {
            return Err(format!(
                "{path}: the identity's `descriptor_hash` is not the descriptor's ({hash})"
            ));
        }
    }
    let mut entries = 0;
    for collection in &hid.collections {
        count_entries(collection, 1, &mut entries)
            .map_err(|message| format!("{path}.hid.collections: {message}"))?;
    }
    if hid.input_samples.len() > MAX_SAMPLES {
        return Err(format!(
            "{path}.hid: {} input samples; at most {MAX_SAMPLES}",
            hid.input_samples.len()
        ));
    }
    for (index, sample) in hid.input_samples.iter().enumerate() {
        let at = format!("{path}.hid.input_samples[{index}]");
        if sample.data.0.len() > MAX_SAMPLE_BYTES {
            return Err(format!(
                "{at}: {} bytes; at most {MAX_SAMPLE_BYTES}",
                sample.data.0.len()
            ));
        }
        if is_identifying_input_report(identity.vendor_id, sample.report_id) {
            return Err(format!(
                "{at}: report {:#04x} of this vendor can carry identifiers",
                sample.report_id
            ));
        }
    }
    Ok(())
}

/// Counts collections, reports, items and usages, and checks the nesting.
fn count_entries(
    collection: &HidCollection,
    depth: usize,
    entries: &mut usize,
) -> Result<(), String> {
    if depth > MAX_HID_NESTING {
        return Err(format!("nested deeper than {MAX_HID_NESTING} levels"));
    }
    *entries += 1;
    for report in collection
        .input_reports
        .iter()
        .chain(&collection.output_reports)
        .chain(&collection.feature_reports)
    {
        *entries += 1;
        for item in &report.items {
            *entries += 1 + item.usages.len();
        }
    }
    if *entries > MAX_HID_ENTRIES {
        return Err(format!(
            "more than {MAX_HID_ENTRIES} collections, reports, items and usages"
        ));
    }
    for child in &collection.children {
        count_entries(child, depth + 1, entries)?;
    }
    Ok(())
}
