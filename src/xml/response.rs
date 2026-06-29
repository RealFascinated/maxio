use quick_xml::se::to_string;
use serde::Serialize;

pub const S3_XMLNS: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

pub fn to_xml<T: Serialize>(value: &T) -> Result<String, String> {
    let inner = to_string(value).map_err(|e| e.to_string())?;
    let with_ns = inject_xmlns(&inner);
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>{}",
        with_ns
    ))
}

fn inject_xmlns(xml: &str) -> String {
    let Some(end) = xml.find('>') else {
        return xml.to_string();
    };
    let tag = &xml[..end];
    if tag.contains("xmlns") {
        return xml.to_string();
    }
    format!("{} xmlns=\"{}\"{}", tag, S3_XMLNS, &xml[end..])
}
