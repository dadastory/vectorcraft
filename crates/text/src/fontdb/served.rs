use super::build::{style_italic, style_weight};
use super::names::norm;
use super::{FontDb, ServedFace};

impl FontDb {
    /// Advertise host-provided faces without loading bytes or requesting them. Repeated
    /// registration keeps the first metadata for a family and style, like loaded faces.
    pub fn register_served_faces(&self, faces: Vec<ServedFace>) {
        let mut changed = false;
        {
            let mut served = self.served.write().unwrap_or_else(|e| e.into_inner());
            for face in faces {
                if !face.family.trim().is_empty()
                    && !face.style.trim().is_empty()
                    && !served.iter().any(|f| f.family.eq_ignore_ascii_case(&face.family) && norm(&f.style) == norm(&face.style))
                {
                    served.push(face);
                    changed = true;
                }
            }
        }
        if changed {
            self.changed();
        }
    }

    /// Drain unresolved face requests. The host coalesces downloads and registers the whole
    /// font file with [`Self::add_font`]. A later lookup can request a failed load again.
    pub fn take_requests(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Whether the requested face's bytes are already loaded; never loads or requests a font.
    pub fn is_loaded(&self, family: &str, style: &str) -> bool {
        let family = norm(family);
        let style = norm(style);
        self.read_faces()
            .iter()
            .any(|f| (norm(&f.family) == family || f.keys.families.contains(&family)) && (norm(&f.style) == style || f.keys.styles.contains(&style)))
    }

    pub(super) fn request_served(&self, family: &str, style: &str) {
        let request = {
            let served = self.served.read().unwrap_or_else(|e| e.into_inner());
            let matches = || served.iter().filter(|f| f.family.eq_ignore_ascii_case(family));
            matches()
                .find(|f| norm(&f.style) == norm(style))
                .or_else(|| {
                    let distance = |f: &ServedFace| {
                        (f32::from(f.weight) - style_weight(style)).abs() + if f.italic != style_italic(style) { 250.0 } else { 0.0 }
                    };
                    matches().min_by(|a, b| distance(a).total_cmp(&distance(b)))
                })
                .map(|f| (f.family.clone(), f.style.clone()))
        };
        if let Some(request) = request
            && !self.is_loaded(&request.0, &request.1)
        {
            let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
            if !requests.contains(&request) {
                requests.push(request);
            }
        }
    }
}

#[cfg(test)]
mod served_fonts_tests {
    use super::*;

    #[test]
    fn metadata_is_lazy_and_loading_a_variable_file_satisfies_its_named_styles() {
        let db = FontDb::with_font_dirs(vec![]);
        let family = crate::test_fonts::VARIABLE_FAMILY;
        let before = db.family_list();
        let generation = db.generation();
        let listed = [("Regular", 400), ("SemiBold", 550), ("Bold", 700)]
            .into_iter()
            .map(|(style, weight)| ServedFace { family: family.into(), style: style.into(), weight, italic: false })
            .collect();
        db.register_served_faces(listed);
        assert!(db.generation() > generation);
        assert!(!before.iter().any(|f| f == family));
        assert!(db.family_list().iter().any(|f| f == family));
        assert!(db.has_family(family));
        assert_eq!(db.styles(family), ["Regular", "SemiBold", "Bold"]);
        assert_eq!(db.canonical(family, "Bold"), (family.into(), "Bold".into()));
        assert!(!db.is_loaded(family, "Bold"));
        assert!(db.take_requests().is_empty());
        db.face(family, "Bold");
        db.face(family, "Bold");
        assert_eq!(db.take_requests(), [(family.into(), "Bold".into())]);
        db.face(family, "Bold");
        assert_eq!(db.take_requests().len(), 1, "a failed request can be retried");
        assert!(db.add_font(crate::test_fonts::variable_font().unwrap()) >= 3);
        for style in ["Regular", "SemiBold", "Bold"] {
            assert!(db.is_loaded(family, style), "{style}");
            assert_eq!(db.face(family, style).unwrap().family, family);
        }
        assert!(db.take_requests().is_empty());
        // An existing face in this family does not satisfy a missing named style.
        db.faces.write().unwrap().retain(|f| f.family != family || f.style != "Bold");
        db.face(family, "Bold");
        assert_eq!(db.take_requests(), [(family.into(), "Bold".into())]);
    }
}
