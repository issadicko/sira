//! Corps : valeur entière, fusion JSON à 3 voies, ou collection à clé pour les formulaires.

use xc_core::Body;

use super::keyed::{self, form_key_value_ident, form_multipart_ident, Group};
use super::{json, settle, Cx, Extra, Field, MergeError, Slot};

pub(super) fn merge(cx: &mut Cx, base: Option<&Body>, ours: &Body, theirs: &Body) -> Result<Body, MergeError> {
    match (ours, theirs) {
        (Body::FormUrlEncoded { fields: own }, Body::FormUrlEncoded { fields: spec }) => {
            let before = base.map(|b| match b {
                Body::FormUrlEncoded { fields } => fields.as_slice(),
                _ => &[],
            });
            let group = Group::plain(Field::Body, form_key_value_ident);
            Ok(Body::FormUrlEncoded { fields: keyed::merge(cx, &group, before, own, spec)? })
        }
        (Body::MultipartForm { fields: own }, Body::MultipartForm { fields: spec }) => {
            let before = base.map(|b| match b {
                Body::MultipartForm { fields } => fields.as_slice(),
                _ => &[],
            });
            let group = Group::plain(Field::Body, form_multipart_ident);
            Ok(Body::MultipartForm { fields: keyed::merge(cx, &group, before, own, spec)? })
        }
        _ => whole(cx, base, ours, theirs),
    }
}

fn whole(cx: &mut Cx, base: Option<&Body>, ours: &Body, theirs: &Body) -> Result<Body, MergeError> {
    let json = match (base, ours, theirs) {
        (Some(Body::Json { data: before }), Body::Json { data: own }, Body::Json { data: spec })
            if own != spec && before != own && before != spec =>
        {
            json::merge(before, own, spec)
        }
        _ => None,
    };
    let clean = json.as_ref().filter(|merged| merged.conflicts == 0);
    let extra = Extra {
        merged: clean.map(|merged| Body::Json { data: merged.text.clone() }),
        both: json.map(|merged| Body::Json { data: merged.text }),
    };
    let editable = is_text(ours) && is_text(theirs);
    let slot = Slot {
        field: Field::Body,
        suffix: "body",
        label: "Corps",
        show,
        edit: editable.then_some(edited as fn(&Body, &str) -> Body),
    };
    settle(cx, &slot, base, ours, theirs, extra)
}

fn is_text(body: &Body) -> bool {
    matches!(body, Body::Json { .. } | Body::Text { .. } | Body::Xml { .. })
}

fn edited(ours: &Body, value: &str) -> Body {
    let data = value.to_owned();
    match ours {
        Body::Json { .. } => Body::Json { data },
        Body::Xml { .. } => Body::Xml { data },
        _ => Body::Text { data },
    }
}

fn show(body: &Body) -> Option<String> {
    match body {
        Body::None => None,
        Body::Json { data } | Body::Text { data } | Body::Xml { data } => Some(data.clone()),
        Body::FormUrlEncoded { .. } => Some("form-urlencoded".into()),
        Body::MultipartForm { .. } => Some("multipart-form".into()),
        Body::Other { label } => Some(label.clone()),
    }
}
