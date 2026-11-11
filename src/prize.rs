//! Preise: frei eingetragen oder als EVE-Item aus den eigenen Assets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Prize {
    /// Anzeigename, z. B. „Gila“ oder „500 Mio. ISK“
    pub name: String,
    pub quantity: u64,
    /// EVE-Typ-ID (für das Item-Icon), falls aus den Assets gewählt
    pub type_id: Option<i64>,
    /// EVE-Item-ID des Assets (zum Wiederfinden), falls aus den Assets gewählt
    pub item_id: Option<i64>,
    /// Wo das Item liegt (Station/Struktur, ggf. Container)
    pub location: Option<String>,
}

impl Prize {
    pub fn label(&self) -> String {
        if self.quantity > 1 {
            format!("{}× {}", fmt_qty(self.quantity), self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn icon(&self) -> Option<String> {
        self.type_id.map(|t| type_icon_url(t, 64))
    }
}

pub fn type_icon_url(type_id: i64, size: u32) -> String {
    format!("https://images.evetech.net/types/{type_id}/icon?size={size}")
}

/// Tausenderpunkte: 1500000 → „1.500.000“
pub fn fmt_qty(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Preis für den Gewinner an Position `index` (0-basiert).
/// Mit Preisliste: Gewinner 1 → Preis 1 usw.; ohne Liste gilt die Beschreibung für alle.
pub fn for_winner(prizes: &[Prize], text: &str, index: usize) -> Option<PrizeView> {
    if !prizes.is_empty() {
        return prizes.get(index).map(PrizeView::from);
    }
    let t = text.trim();
    (!t.is_empty()).then(|| PrizeView {
        label: t.to_string(),
        icon: None,
    })
}

/// Kurzfassung „was gibt es zu gewinnen“ für Overlay, Chat und Dock.
pub fn summary(prizes: &[Prize], text: &str) -> String {
    let t = text.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    let labels: Vec<String> = prizes.iter().map(Prize::label).collect();
    match labels.len() {
        0 => String::new(),
        1..=4 => labels.join(", "),
        n => t!("{a1} und {a2} weitere Preise", a1 = labels[..3].join(", "), a2 = n - 3),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PrizeView {
    pub label: String,
    pub icon: Option<String>,
}

impl From<&Prize> for PrizeView {
    fn from(p: &Prize) -> Self {
        Self {
            label: p.label(),
            icon: p.icon(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, q: u64) -> Prize {
        Prize {
            name: name.into(),
            quantity: q,
            type_id: Some(11),
            ..Default::default()
        }
    }

    #[test]
    fn mapping_and_summary() {
        let list = vec![p("Gila", 1), p("Nanite Repair Paste", 1500)];
        assert_eq!(for_winner(&list, "", 0).unwrap().label, "Gila");
        assert_eq!(
            for_winner(&list, "", 1).unwrap().label,
            "1.500× Nanite Repair Paste"
        );
        assert!(for_winner(&list, "x", 2).is_none());
        assert_eq!(
            for_winner(&[], "je 100 Mio. ISK", 5).unwrap().label,
            "je 100 Mio. ISK"
        );
        assert!(for_winner(&[], " ", 0).is_none());
        assert_eq!(summary(&list, ""), "Gila, 1.500× Nanite Repair Paste");
        assert_eq!(summary(&list, "Schiffe!"), "Schiffe!");
        let many: Vec<Prize> = (0..6).map(|i| p(&format!("I{i}"), 1)).collect();
        assert_eq!(summary(&many, ""), "I0, I1, I2 und 3 weitere Preise");
        assert!(list[0].icon().unwrap().contains("/types/11/icon"));
    }
}
