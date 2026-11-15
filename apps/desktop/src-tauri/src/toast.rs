//! Windows notifications ([ADR 0087] §2): kilnd's `notification` events shown as toasts under
//! the app's identifier, which an installed Kiln's Start menu shortcut carries. Where Windows
//! refuses (a build run from its folder), the app's list still has them.
//!
//! [ADR 0087]: ../../../../docs/adr/0087-notifications.md

/// Text as it may sit in the toast's XML.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// The toast's XML: a title line and a body line.
#[must_use]
pub fn xml(title: &str, body: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    )
}

/// Shows a toast as the app `app_id`.
///
/// # Errors
/// Windows refused it (no shortcut with that id, notifications off) or COM couldn't start.
#[cfg(windows)]
pub fn show(app_id: &str, title: &str, body: &str) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;
    kiln_sys::win::com_multithreaded().map_err(|e| e.to_string())?;
    let doc = XmlDocument::new().map_err(|e| e.to_string())?;
    doc.LoadXml(&HSTRING::from(xml(title, body)))
        .map_err(|e| e.to_string())?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.to_string())?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
        .and_then(|notifier| notifier.Show(&toast))
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn show(_app_id: &str, _title: &str, _body: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_from_an_email_cant_change_the_toast() {
        assert_eq!(
            xml("Mail caught: <b>Hi</b> & \"you\"", "From a@x.test"),
            "<toast><visual><binding template=\"ToastGeneric\"><text>Mail caught: &lt;b&gt;Hi&lt;/b&gt; &amp; &quot;you&quot;</text><text>From a@x.test</text></binding></visual></toast>"
        );
        assert_eq!(escape("bell\u{7}"), "bell");
    }
}
