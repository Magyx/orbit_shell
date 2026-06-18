use ui::{
    el,
    model::{Color, Size, Vec4, Wrap},
    theme::Theme,
    widget::{Button, Column, Element, Length, Row, Scrollable, Spacer, Text, TextField},
};
use yaml_serde::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Field(String),
    Index(usize),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path(pub Vec<Seg>);

impl Path {
    fn child(&self, seg: Seg) -> Path {
        let mut v = self.0.clone();
        v.push(seg);
        Path(v)
    }
}

fn value_at_mut<'v>(root: &'v mut Value, path: &Path) -> Option<&'v mut Value> {
    let mut cur = root;
    for seg in &path.0 {
        cur = match (cur, seg) {
            // VERIFY: Mapping keyed by Value; Sequence indexable.
            (Value::Mapping(m), Seg::Field(k)) => m.get_mut(Value::from(k.as_str()))?,
            (Value::Sequence(s), Seg::Index(i)) => s.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn value_at<'v>(root: &'v Value, path: &Path) -> Option<&'v Value> {
    let mut cur = root;
    for seg in &path.0 {
        cur = match (cur, seg) {
            (Value::Mapping(m), Seg::Field(k)) => m.get(Value::from(k.as_str()))?,
            (Value::Sequence(s), Seg::Index(i)) => s.get(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

#[derive(Clone, Debug)]
pub enum Shape {
    Bool,
    Integer {
        min: Option<i64>,
        max: Option<i64>,
    },
    Number,
    Str,
    Enum {
        variants: Vec<String>,
    },
    Optional(Box<Shape>),
    Struct {
        fields: Vec<(String, Shape)>,
    },
    Seq(Box<Shape>),
    /// oneOf / tagged unions / cycle breaks - opt-in handles these in v1.
    Opaque,
}

/// Normalize a schemars schema (already serde-json) into a Shape, resolving
/// `$ref` against `$defs`. Recursive refs are cycle-broken to Opaque for now;
/// the Box nodes keep the type representable for the richer case later.
pub fn shape_from_schema(root: &serde_json::Value) -> Shape {
    let mut visiting: Vec<String> = Vec::new();
    shape_of(root, root, &mut visiting)
}

fn schema_defs(root: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    root.get("$defs").and_then(|d| d.as_object())
}

fn deref<'a>(
    node: &'a serde_json::Value,
    root: &'a serde_json::Value,
) -> (Option<String>, &'a serde_json::Value) {
    if let Some(r) = node.get("$ref").and_then(|r| r.as_str()) {
        let name = r.rsplit('/').next().unwrap_or(r).to_string();
        if let Some(t) = schema_defs(root).and_then(|d| d.get(&name)) {
            return (Some(name), t);
        }
    }
    (None, node)
}

fn shape_of(
    node: &serde_json::Value,
    root: &serde_json::Value,
    visiting: &mut Vec<String>,
) -> Shape {
    let (ref_name, node) = deref(node, root);
    if let Some(name) = &ref_name {
        if visiting.iter().any(|n| n == name) {
            return Shape::Opaque; // cycle break (v1)
        }
        visiting.push(name.clone());
    }
    let shape = classify(node, root, visiting);
    if ref_name.is_some() {
        visiting.pop();
    }
    shape
}

fn type_str(node: &serde_json::Value) -> Option<&str> {
    match node.get("type") {
        Some(serde_json::Value::String(s)) => Some(s.as_str()),
        Some(serde_json::Value::Array(a)) => {
            a.iter().filter_map(|v| v.as_str()).find(|s| *s != "null")
        }
        _ => None,
    }
}

fn optional_inner(
    node: &serde_json::Value,
    root: &serde_json::Value,
    visiting: &mut Vec<String>,
) -> Option<Shape> {
    // type: ["T", "null"]
    if let Some(serde_json::Value::Array(a)) = node.get("type")
        && a.iter().any(|v| v.as_str() == Some("null"))
        && a.len() > 1
    {
        return Some(classify(node, root, visiting));
    }
    // anyOf: [ T, { type: "null" } ]
    if let Some(serde_json::Value::Array(any)) = node.get("anyOf") {
        let has_null = any
            .iter()
            .any(|v| v.get("type").and_then(|t| t.as_str()) == Some("null"));
        if has_null
            && let Some(t) = any
                .iter()
                .find(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
        {
            return Some(shape_of(t, root, visiting));
        }
    }
    None
}

fn classify(
    node: &serde_json::Value,
    root: &serde_json::Value,
    visiting: &mut Vec<String>,
) -> Shape {
    if let Some(en) = node.get("enum").and_then(|e| e.as_array()) {
        let variants: Vec<String> = en
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if !variants.is_empty() {
            return Shape::Enum { variants };
        }
    }
    if let Some(inner) = optional_inner(node, root, visiting) {
        return Shape::Optional(Box::new(inner));
    }
    match type_str(node) {
        Some("boolean") => Shape::Bool,
        Some("integer") => Shape::Integer {
            min: node.get("minimum").and_then(|v| v.as_i64()),
            max: node.get("maximum").and_then(|v| v.as_i64()),
        },
        Some("number") => Shape::Number,
        Some("string") => Shape::Str,
        Some("array") => Shape::Seq(Box::new(
            node.get("items")
                .map(|it| shape_of(it, root, visiting))
                .unwrap_or(Shape::Opaque),
        )),
        Some("object") => {
            let mut fields = Vec::new();
            if let Some(props) = node.get("properties").and_then(|p| p.as_object()) {
                for (k, v) in props {
                    fields.push((k.clone(), shape_of(v, root, visiting)));
                }
            }
            Shape::Struct { fields }
        }
        _ => Shape::Opaque,
    }
}

#[derive(Clone, Debug)]
pub enum AutoSettingsMsg {
    SetBool { path: Path, value: bool },
    SetEnum { path: Path, variant: String },
    SetText { path: Path, value: String },
}

pub fn auto_settings_view(
    name: &'static str,
    shape: &Shape,
    effective: &Value,
    theme: &Theme,
) -> Element<AutoSettingsMsg> {
    let body = render(shape, effective, &Path::default(), theme);
    Scrollable::new(
        Column::new(el![Text::h3(pretty(name)), body])
            .spacing(8)
            .padding(Vec4::splat(16))
            .size(Size::splat(Length::Grow)),
    )
    .size(Size::splat(Length::Grow))
    .into()
}

fn render(shape: &Shape, value: &Value, path: &Path, theme: &Theme) -> Element<AutoSettingsMsg> {
    match shape {
        Shape::Bool => {
            let cur = value.as_bool().unwrap_or(false);
            // Fix: Toggle widget (bound bool, on_toggle(bool)->M). Button stands in.
            Button::new_with(Text::body(if cur { "on" } else { "off" }))
                .color(if cur {
                    Color::rgba(60, 120, 220, 220)
                } else {
                    Color::rgba(45, 45, 52, 220)
                })
                .on_press(AutoSettingsMsg::SetBool {
                    path: path.clone(),
                    value: !cur,
                })
                .size(Size::new(Length::Grow, Length::Fit))
                .into()
        }
        Shape::Integer { .. } | Shape::Number => {
            // Fix: NumberInput/Stepper (typed value + min/max/step); with
            // Integer{min,max} a Slider becomes viable. Commit on submit to avoid
            // re-render churn fighting keystrokes.
            let p = path.clone();
            TextField::<AutoSettingsMsg>::new(
                number_str(value),
                Size::new(Length::Grow, Length::Fit),
            )
            .on_change(move |s: &str| AutoSettingsMsg::SetText {
                path: p.clone(),
                value: s.to_owned(),
            })
            .into()
        }
        Shape::Str => {
            let p = path.clone();
            let cur = value.as_str().unwrap_or_default().to_owned();
            TextField::<AutoSettingsMsg>::new(cur, Size::new(Length::Grow, Length::Fit))
                .on_change(move |s: &str| AutoSettingsMsg::SetText {
                    path: p.clone(),
                    value: s.to_owned(),
                })
                .into()
        }
        Shape::Enum { variants } => {
            // Fix: Dropdown/List widget. Button row stands in.
            let cur = value.as_str().unwrap_or_default();
            let mut row = Row::empty().spacing(6);
            for v in variants {
                let selected = v == cur;
                row.push(
                    Button::new_with(Text::body(v.clone()))
                        .color(if selected {
                            Color::rgba(60, 120, 220, 220)
                        } else {
                            Color::rgba(45, 45, 52, 220)
                        })
                        .on_press(AutoSettingsMsg::SetEnum {
                            path: path.clone(),
                            variant: v.clone(),
                        }),
                );
            }
            row.into()
        }
        Shape::Optional(inner) => {
            // Fix: nullable toggle (enable/disable) seeding inner's default.
            if value.is_null() {
                Text::label("(unset)").into()
            } else {
                render(inner, value, path, theme)
            }
        }
        Shape::Struct { fields } => {
            let mut col = Column::empty()
                .spacing(6)
                .size(Size::new(Length::Grow, Length::Fit));

            for (name, fshape) in fields {
                let child_path = path.child(Seg::Field(name.clone()));
                let child_val = value_at(value, &Path(vec![Seg::Field(name.clone())]))
                    .cloned()
                    .unwrap_or(Value::Null);
                col.push(labeled(
                    &pretty(name),
                    render(fshape, &child_val, &child_path, theme),
                ));
            }
            col.into()
        }
        Shape::Seq(_) | Shape::Opaque => {
            // Fix: list editor (add/remove/reorder) + tagged-union editor.
            Text::label("(not editable inline — provide a ModuleSettings impl)").into()
        }
    }
}

fn labeled(label: &str, control: Element<AutoSettingsMsg>) -> Element<AutoSettingsMsg> {
    Row::new(el![
        Text::body(label.to_owned())
            .wrap(Wrap::None)
            .size(Size::new(Length::Grow, Length::Fit)),
        Row::new([control]).size(Size::splat(Length::Grow))
    ])
    .padding(Vec4::splat(6))
    .size(Size::new(Length::Grow, Length::Fit))
    .into()
}

pub fn auto_settings_update(shape: &Shape, root: &mut Value, msg: &AutoSettingsMsg) -> bool {
    match msg {
        AutoSettingsMsg::SetBool { path, value } => set_leaf(root, path, Value::from(*value)),
        AutoSettingsMsg::SetEnum { path, variant } => {
            set_leaf(root, path, Value::from(variant.clone()))
        }
        AutoSettingsMsg::SetText { path, value } => match shape_at(shape, path) {
            Some(Shape::Integer { .. }) | Some(Shape::Number) => {
                match value.trim().parse::<f64>() {
                    Ok(n) if n.fract() == 0.0 => set_leaf(root, path, Value::from(n as i64)),
                    Ok(n) => set_leaf(root, path, Value::from(n)),
                    Err(_) => false,
                }
            }
            _ => set_leaf(root, path, Value::from(value.clone())),
        },
    }
}

fn set_leaf(root: &mut Value, path: &Path, new: Value) -> bool {
    match value_at_mut(root, path) {
        Some(slot) if *slot != new => {
            *slot = new;
            true
        }
        _ => false,
    }
}

fn shape_at<'s>(shape: &'s Shape, path: &Path) -> Option<&'s Shape> {
    let mut cur = shape;
    for seg in &path.0 {
        cur = match (cur, seg) {
            (Shape::Struct { fields }, Seg::Field(k)) => &fields.iter().find(|(n, _)| n == k)?.1,
            (Shape::Seq(item), Seg::Index(_)) => item.as_ref(),
            (Shape::Optional(inner), _) => return shape_at(inner, &Path(vec![seg.clone()])),
            _ => return None,
        };
    }
    Some(cur)
}

fn number_str(v: &Value) -> String {
    if let Some(i) = v.as_i64() {
        i.to_string()
    } else if let Some(f) = v.as_f64() {
        f.to_string()
    } else {
        String::new()
    }
}

fn pretty(key: &str) -> String {
    let mut out = String::new();
    for (i, w) in key.split('_').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut c = w.chars();
        if let Some(f) = c.next() {
            out.extend(f.to_uppercase());
            out.push_str(c.as_str());
        }
    }
    out
}
