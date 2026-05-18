use wallpaper_core::DisplaySnapshotEntry;

pub trait DisplayLabelExt {
    fn title(&self) -> String;
    fn title_with_role(&self, primary: bool) -> String;
    fn title_with_suffix(&self, suffix: &str) -> String;
}

impl DisplayLabelExt for DisplaySnapshotEntry {
    fn title(&self) -> String {
        self.title_with_suffix(&format!("({})", self.desc.display_id))
    }

    fn title_with_role(&self, primary: bool) -> String {
        if primary {
            self.title_with_suffix(&format!("({} - Primary)", self.desc.display_id))
        } else {
            self.title()
        }
    }

    fn title_with_suffix(&self, suffix: &str) -> String {
        let name = self.identity.name.as_deref().unwrap_or("Display");
        let vendor_model = match (self.identity.vendor_id, self.identity.model_id) {
            (Some(vendor), Some(model)) => Some(format!("Vendor {vendor} - Model {model}")),
            (Some(vendor), None) => Some(format!("Vendor {vendor}")),
            (None, Some(model)) => Some(format!("Model {model}")),
            (None, None) => None,
        };

        match vendor_model {
            Some(vendor_model) if name == "Display" => format!("{vendor_model} {suffix}"),
            Some(vendor_model) => format!("{name} - {vendor_model} {suffix}"),
            None => format!("{name} {suffix}"),
        }
    }
}
