//! Port de `content-type.js` : familles de types de contenu reconnues par l'import cURL.

fn normalize(content_type: Option<&str>) -> String {
    content_type.map(str::to_lowercase).unwrap_or_default()
}

pub(crate) fn is_ndjson_like(content_type: Option<&str>) -> bool {
    let normalized = normalize(content_type);
    normalized.contains("application/x-ndjson") || normalized.contains("application/ndjson")
}

pub(crate) fn is_json_like(content_type: Option<&str>) -> bool {
    let normalized = normalize(content_type);
    normalized.contains("application/json") || normalized.contains("+json")
}

pub(crate) fn is_xml_like(content_type: Option<&str>) -> bool {
    let normalized = normalize(content_type);
    normalized.contains("application/xml") || normalized.contains("+xml") || normalized.contains("text/xml")
}

pub(crate) fn is_plain_text(content_type: Option<&str>) -> bool {
    normalize(content_type).contains("text/plain")
}

pub(crate) fn is_structured_content_type(content_type: Option<&str>) -> bool {
    is_ndjson_like(content_type)
        || is_json_like(content_type)
        || is_xml_like(content_type)
        || is_plain_text(content_type)
}
