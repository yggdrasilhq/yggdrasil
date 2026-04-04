use maker_model::{BuildProfile, PresetId};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PresetCard {
    pub id: PresetId,
    pub title: &'static str,
    pub summary: &'static str,
    pub recommended_profile: BuildProfile,
}

const PRESET_CARDS: [PresetCard; 4] = [
    PresetCard {
        id: PresetId::Nas,
        title: "NAS",
        summary: "Quiet storage spine with the server profile and conservative defaults.",
        recommended_profile: BuildProfile::Server,
    },
    PresetCard {
        id: PresetId::DevHost,
        title: "Dev Host",
        summary: "KDE-first workstation flow for the person commissioning and iterating on the box.",
        recommended_profile: BuildProfile::Kde,
    },
    PresetCard {
        id: PresetId::PersonalWorkstation,
        title: "Personal Workstation",
        summary: "A guided KDE build that still emits the native ygg config you can keep editing.",
        recommended_profile: BuildProfile::Kde,
    },
    PresetCard {
        id: PresetId::RecoveryAnchor,
        title: "Recovery Anchor",
        summary: "Server profile with first-boot pragmatism for the machine you want available when things go wrong.",
        recommended_profile: BuildProfile::Server,
    },
];

pub fn preset_cards() -> &'static [PresetCard] {
    &PRESET_CARDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_titles_stay_unique() {
        let mut titles = preset_cards()
            .iter()
            .map(|card| card.title)
            .collect::<Vec<_>>();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), preset_cards().len());
    }
}
