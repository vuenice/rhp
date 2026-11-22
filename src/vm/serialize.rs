//! JSON-envelope `serialize` / `unserialize`.
//!
//! Objects become `{"__class","__data"}` from promoted constructor properties.
//! A value that defines `__serialize` is replaced by that method's return value
//! (model tags such as `{"_m","id"}`). Dates become `{"_d": ISO-8601}`.
//! On the way back, missing constructor keys use parameter defaults, unknown
//! keys are ignored, `_d` calls `Time::parse`, and `_m` calls `Class::findOrFail`
//! or `__unserialize`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use serde_json::{Map, Number, Value};

use crate::ast::types::Visibility;
use crate::ir::CompiledParam;
use crate::lexer::span::Span;
use crate::type_system::types::{CollectionSubtype, RhpType};
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::time::dispatch_time;
use crate::vm::value::{PhpValue, VmObject};

use super::{StagedArg, VM};

const MAX_DEPTH: usize = 64;

impl VM {
    pub(crate) fn serialize_value(&mut self, value: &PhpValue, span: Span) -> Result<String, RuntimeError> {
        let mut stack = Vec::new();
        let json = self.encode_value(value, 0, &mut stack, span)?;
        Ok(json.write())
    }

    pub(crate) fn unserialize_value(&mut self, payload: &str, span: Span) -> Result<PhpValue, RuntimeError> {
        let trimmed = payload.trim();
        if trimmed.is_empty() {
            return Err(self.ser_err(span, "Invalid serialized payload: empty string"));
        }
        let json: Value = serde_json::from_str(trimmed).map_err(|err| {
            self.ser_err(span, format!("Invalid serialized payload: {err}"))
        })?;
        self.hydrate(&json, span)
    }

    fn encode_value(
        &mut self,
        value: &PhpValue,
        depth: usize,
        stack: &mut Vec<usize>,
        span: Span,
    ) -> Result<Json, RuntimeError> {
        if depth > MAX_DEPTH {
            return Err(self.ser_err(span, "Serialized value is nested too deeply"));
        }
        match value {
            PhpValue::Null => Ok(Json::Null),
            PhpValue::Bool(b) => Ok(Json::Bool(*b)),
            PhpValue::Int(n) => Ok(Json::Int(*n)),
            PhpValue::Float(f) => {
                if Number::from_f64(*f).is_none() {
                    return Err(self.ser_err(span, "Cannot serialize non-finite float"));
                }
                Ok(Json::Float(*f))
            }
            PhpValue::String(s) => Ok(Json::String(s.to_string())),
            PhpValue::Rune(c) => Ok(Json::String(c.to_string())),
            PhpValue::EmptyArray => Ok(Json::Array(Vec::new())),
            PhpValue::Array(items) | PhpValue::List(items) => {
                let cloned = items.borrow().clone();
                let mut out = Vec::with_capacity(cloned.len());
                for item in &cloned {
                    out.push(self.encode_value(item, depth + 1, stack, span)?);
                }
                Ok(Json::Array(out))
            }
            PhpValue::Dict(map) => {
                let pairs: Vec<(String, PhpValue)> = map
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut out = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    out.push((k, self.encode_value(&v, depth + 1, stack, span)?));
                }
                Ok(Json::Object(out))
            }
            PhpValue::Object(rc) => self.encode_object(&rc.clone(), depth, stack, span),
            PhpValue::Closure(_) => Err(self.ser_err(span, "Cannot serialize closure")),
            PhpValue::Resource(_) => Err(self.ser_err(span, "Cannot serialize resource")),
            PhpValue::Future(_) => Err(self.ser_err(span, "Cannot serialize future")),
            PhpValue::Bytes(_) => Err(self.ser_err(span, "Cannot serialize bytes")),
            PhpValue::Set(_) => Err(self.ser_err(span, "Cannot serialize set")),
            PhpValue::HashMap(_) => Err(self.ser_err(span, "Cannot serialize hashMap")),
            PhpValue::SharedObject(_)
            | PhpValue::SharedArray(_)
            | PhpValue::SharedAtomic(_)
            | PhpValue::SharedValue(_) => {
                Err(self.ser_err(span, "Cannot serialize shared value"))
            }
        }
    }

    fn encode_object(
        &mut self,
        rc: &Rc<RefCell<VmObject>>,
        depth: usize,
        stack: &mut Vec<usize>,
        span: Span,
    ) -> Result<Json, RuntimeError> {
        let ptr = Rc::as_ptr(rc) as usize;
        if stack.contains(&ptr) {
            let name = rc.borrow().class_name.clone();
            return Err(self.ser_err(
                span,
                format!("Cannot serialize circular reference in class {name}"),
            ));
        }
        stack.push(ptr);
        let encoded = self.encode_object_body(rc, depth, stack, span);
        stack.pop();
        encoded
    }

    fn encode_object_body(
        &mut self,
        rc: &Rc<RefCell<VmObject>>,
        depth: usize,
        stack: &mut Vec<usize>,
        span: Span,
    ) -> Result<Json, RuntimeError> {
        let class_name = rc.borrow().class_name.clone();
        if class_name == "WeakReference" {
            return Err(self.ser_err(span, "Cannot serialize weak reference"));
        }

        if self.module.enums.contains_key(&class_name) {
            let case = rc
                .borrow()
                .get_property("name")
                .map(|v| v.to_php_string())
                .unwrap_or_default();
            return Ok(Json::Object(vec![
                ("__enum".to_string(), Json::String(class_name)),
                ("__case".to_string(), Json::String(case)),
            ]));
        }

        if let Some(iso) = date_iso(&rc.borrow()) {
            return Ok(Json::Object(vec![("_d".to_string(), Json::String(iso))]));
        }

        if self.resolve_method_id(&class_name, "__serialize").is_some() {
            let ret = self.invoke_method_sync(
                PhpValue::Object(rc.clone()),
                "__serialize",
                Vec::new(),
                span,
            )?;
            return self.encode_value(&ret, depth + 1, stack, span);
        }

        let keys = self.schema_keys(&class_name);
        let values: Vec<(String, PhpValue)> = {
            let obj = rc.borrow();
            keys.into_iter()
                .map(|key| {
                    let val = obj.get_property(&key).cloned().unwrap_or(PhpValue::Null);
                    (key, val)
                })
                .collect()
        };
        let mut data = Vec::with_capacity(values.len());
        for (key, val) in values {
            data.push((key, self.encode_value(&val, depth + 1, stack, span)?));
        }
        Ok(Json::Object(vec![
            ("__class".to_string(), Json::String(class_name)),
            ("__data".to_string(), Json::Object(data)),
        ]))
    }

    fn schema_keys(&self, class_name: &str) -> Vec<String> {
        if let Some(params) = self.constructor_params(class_name) {
            let promoted: Vec<String> = params
                .iter()
                .filter(|p| p.visibility.is_some())
                .map(|p| bare_name(&p.name).to_string())
                .collect();
            if !promoted.is_empty() {
                return promoted;
            }
        }
        self.public_fields(class_name)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn constructor_params(&self, class_name: &str) -> Option<Vec<CompiledParam>> {
        let id = self.resolve_method_id(class_name, "__construct")?;
        self.module.get_function(id).map(|func| func.params.clone())
    }

    fn public_fields(&self, class_name: &str) -> Vec<(String, RhpType)> {
        let Some(info) = self.module.get_class_info_by_name(class_name) else {
            return Vec::new();
        };
        info.prop_layout
            .iter()
            .filter(|prop| !prop.is_static && prop.visibility == Visibility::Public)
            .map(|prop| (prop.name.to_string(), prop.rhp_type.clone()))
            .collect()
    }

    fn has_property(&self, class_name: &str, name: &str) -> bool {
        self.module
            .get_class_info_by_name(class_name)
            .map(|info| info.prop_indices.contains_key(name))
            .unwrap_or(false)
    }

    fn hydrate(&mut self, value: &Value, span: Span) -> Result<PhpValue, RuntimeError> {
        match value {
            Value::Null => Ok(PhpValue::Null),
            Value::Bool(b) => Ok(PhpValue::Bool(*b)),
            Value::Number(n) => Ok(json_number(n)),
            Value::String(s) => Ok(PhpValue::string(s)),
            Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.hydrate(item, span)?);
                }
                Ok(PhpValue::list(out))
            }
            Value::Object(map) => {
                if let (Some(enum_name), Some(case)) = (
                    map.get("__enum").and_then(Value::as_str),
                    map.get("__case").and_then(Value::as_str),
                ) {
                    return self.restore_enum(enum_name, case, span);
                }
                if let Some(class_name) = map.get("__class").and_then(Value::as_str) {
                    let data = match map.get("__data") {
                        Some(Value::Object(data)) => data.clone(),
                        None | Some(Value::Null) => Map::new(),
                        Some(_) => {
                            return Err(self.ser_err(
                                span,
                                format!("Serialized class {class_name} has a non-object __data field"),
                            ));
                        }
                    };
                    return self.reconstruct(class_name, &data, span);
                }
                if let Some(class_name) = map.get("_m_coll").and_then(Value::as_str) {
                    return self.restore_collection(class_name, map, span);
                }
                if let Some(class_name) = map.get("_m").and_then(Value::as_str) {
                    return self.restore_model(class_name, map, span);
                }
                if map.len() == 1 {
                    if let Some(Value::String(iso)) = map.get("_d") {
                        return self.restore_date(iso, span);
                    }
                }
                let mut entries = Vec::with_capacity(map.len());
                for (key, child) in map {
                    entries.push((key.clone(), self.hydrate(child, span)?));
                }
                Ok(PhpValue::dict(entries))
            }
        }
    }

    fn reconstruct(
        &mut self,
        class_name: &str,
        data: &Map<String, Value>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let clean = class_name.trim().trim_start_matches('\\');
        if self.module.get_class_by_name(clean).is_none() {
            return Err(self.ser_err(span, format!("Unknown class {clean} in serialized payload")));
        }
        if let Some(info) = self.module.get_class_info_by_name(clean) {
            if info.is_abstract {
                return Err(self.ser_err(span, format!("Cannot unserialize abstract class {clean}")));
            }
            if info.is_interface || info.is_trait {
                return Err(self.ser_err(span, format!("Cannot unserialize {clean}")));
            }
        }

        let params = self.constructor_params(clean).unwrap_or_default();
        let promoted_schema = params.iter().any(|p| p.visibility.is_some());
        let mut staged = Vec::with_capacity(params.len());
        let mut null_overrides = Vec::new();
        let mut supplied = HashSet::new();

        for param in &params {
            if param.is_reference {
                return Err(self.ser_err(
                    span,
                    format!(
                        "Cannot unserialize {clean}: constructor parameter ${} is a reference",
                        bare_name(&param.name)
                    ),
                ));
            }
            let key = bare_name(&param.name).to_string();
            let promoted = param.visibility.is_some();
            if param.is_variadic {
                let list = if promoted_schema && promoted {
                    if let Some(raw) = data.get(&key) {
                        supplied.insert(key);
                        self.hydrate(raw, span)?
                    } else {
                        PhpValue::list(Vec::new())
                    }
                } else {
                    PhpValue::list(Vec::new())
                };
                staged.push(arg_unpack(list));
                continue;
            }

            let read_payload = !promoted_schema || promoted;
            if read_payload {
                if let Some(raw) = data.get(&key) {
                    let hydrated = self.hydrate(raw, span)?;
                    let casted = self.cast_param(&param.rhp_type, hydrated, &key, clean, span)?;
                    if casted.is_null() && param.has_default {
                        null_overrides.push(key.clone());
                    }
                    supplied.insert(key);
                    staged.push(arg_value(casted));
                    continue;
                }
            }

            if param.has_default {
                staged.push(arg_skip());
                continue;
            }
            if type_accepts_null(&param.rhp_type) {
                staged.push(arg_value(PhpValue::Null));
                continue;
            }
            if promoted || !promoted_schema {
                return Err(self.ser_err(
                    span,
                    format!("Missing required parameter ${key} while unserializing {clean}"),
                ));
            }
            return Err(self.ser_err(
                span,
                format!(
                    "Cannot unserialize {clean}: constructor parameter ${key} is not a serialized property"
                ),
            ));
        }

        let instance = if self.resolve_method_id(clean, "__construct").is_some() {
            self.call_constructor(clean, &staged, span)?
        } else {
            self.instantiate_class(clean, span)?
        };

        if !null_overrides.is_empty() {
            let pairs: Vec<(String, PhpValue)> = null_overrides
                .into_iter()
                .filter(|name| self.has_property(clean, name))
                .map(|name| (name, PhpValue::Null))
                .collect();
            write_props(&instance, &pairs);
        }

        if !promoted_schema {
            self.overlay_public(&instance, clean, data, &supplied, span)?;
        }
        Ok(instance)
    }

    fn overlay_public(
        &mut self,
        instance: &PhpValue,
        class_name: &str,
        data: &Map<String, Value>,
        skip: &HashSet<String>,
        span: Span,
    ) -> Result<(), RuntimeError> {
        let fields = self.public_fields(class_name);
        let mut pairs = Vec::new();
        for (name, ty) in fields {
            if skip.contains(&name) {
                continue;
            }
            let Some(raw) = data.get(&name) else {
                continue;
            };
            let hydrated = self.hydrate(raw, span)?;
            let casted = self.cast_param(&ty, hydrated, &name, class_name, span)?;
            pairs.push((name, casted));
        }
        write_props(instance, &pairs);
        Ok(())
    }

    fn call_constructor(
        &mut self,
        class_name: &str,
        staged: &[StagedArg],
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let instance = self.instantiate_class(class_name, span)?;
        let ctor_id = self
            .resolve_method_id(class_name, "__construct")
            .expect("constructor id");
        let depth = self.frames.len();
        self.push_compiled_call_frame(
            ctor_id,
            Some(instance.clone()),
            staged,
            &[],
            None,
            Some(class_name.to_string()),
            span,
        )?;
        self.dispatch_loop(depth)?;
        Ok(instance)
    }

    fn restore_model(
        &mut self,
        class_name: &str,
        data: &Map<String, Value>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let clean = class_name.trim().trim_start_matches('\\');
        if self.resolve_method_id(clean, "findOrFail").is_some() {
            let id = match data.get("id") {
                Some(raw) => self.to_plain(raw, span)?,
                None => {
                    return Err(self.ser_err(
                        span,
                        format!("Model reference for {clean} is missing id"),
                    ));
                }
            };
            return self.invoke_static_method_sync(clean, "findOrFail", vec![id], span);
        }
        if self.resolve_method_id(clean, "__unserialize").is_some() {
            if self.module.get_class_by_name(clean).is_none() {
                return Err(self.ser_err(span, format!("Unknown class {clean} in serialized payload")));
            }
            let instance = self.instantiate_class(clean, span)?;
            let payload = self.to_plain(&Value::Object(data.clone()), span)?;
            self.invoke_method_sync(instance.clone(), "__unserialize", vec![payload], span)?;
            return Ok(instance);
        }
        Err(self.ser_err(
            span,
            format!("Class {clean} has no findOrFail() or __unserialize() to restore a model reference"),
        ))
    }

    fn restore_collection(
        &mut self,
        class_name: &str,
        data: &Map<String, Value>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let clean = class_name.trim().trim_start_matches('\\');
        let ids = match data.get("ids") {
            Some(raw) => self.to_plain(raw, span)?,
            None => {
                return Err(self.ser_err(
                    span,
                    format!("Model collection for {clean} is missing ids"),
                ));
            }
        };
        if self.resolve_method_id(clean, "whereIn").is_none() {
            return Err(self.ser_err(
                span,
                format!("Class {clean} has no whereIn() to restore a model collection"),
            ));
        }
        let builder = self.invoke_static_method_sync(
            clean,
            "whereIn",
            vec![PhpValue::string("id"), ids],
            span,
        )?;
        match &builder {
            PhpValue::Object(_) | PhpValue::SharedObject(_) => {
                self.invoke_method_sync(builder, "get", Vec::new(), span)
            }
            other => Err(self.ser_err(
                span,
                format!(
                    "whereIn() on {clean} returned {}, which has no get()",
                    other.type_name()
                ),
            )),
        }
    }

    fn restore_date(&self, iso: &str, span: Span) -> Result<PhpValue, RuntimeError> {
        dispatch_time("parse", &[PhpValue::string(iso)]).map_err(|kind| self.make_error(kind, span))
    }

    fn restore_enum(&self, enum_name: &str, case: &str, span: Span) -> Result<PhpValue, RuntimeError> {
        let clean = enum_name.trim().trim_start_matches('\\');
        self.class_constants
            .get(&(clean.to_string(), case.to_string()))
            .cloned()
            .ok_or_else(|| {
                self.ser_err(span, format!("Unknown enum case {clean}::{case} in serialized payload"))
            })
    }

    fn to_plain(&self, value: &Value, span: Span) -> Result<PhpValue, RuntimeError> {
        match value {
            Value::Null => Ok(PhpValue::Null),
            Value::Bool(b) => Ok(PhpValue::Bool(*b)),
            Value::Number(n) => Ok(json_number(n)),
            Value::String(s) => Ok(PhpValue::string(s)),
            Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.to_plain(item, span)?);
                }
                Ok(PhpValue::list(out))
            }
            Value::Object(map) => {
                let mut entries = Vec::with_capacity(map.len());
                for (key, child) in map {
                    entries.push((key.clone(), self.to_plain(child, span)?));
                }
                Ok(PhpValue::dict(entries))
            }
        }
    }

    fn cast_param(
        &mut self,
        ty: &RhpType,
        val: PhpValue,
        param: &str,
        class_name: &str,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let produced = self.coerce(ty, val, param, class_name, span)?;
        if self.value_matches(&produced, ty) {
            Ok(produced)
        } else {
            Err(self.ser_err(
                span,
                format!(
                    "Cannot cast {} to {ty} for parameter ${param} of {class_name}",
                    produced.type_name()
                ),
            ))
        }
    }

    fn coerce(
        &mut self,
        ty: &RhpType,
        val: PhpValue,
        param: &str,
        class_name: &str,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        match ty {
            RhpType::Mixed | RhpType::Dynamic => Ok(val),
            RhpType::Nullable(inner) => {
                if val.is_null() {
                    Ok(PhpValue::Null)
                } else {
                    self.coerce(inner, val, param, class_name, span)
                }
            }
            RhpType::Union(items) => {
                if val.is_null() && type_accepts_null(ty) {
                    return Ok(PhpValue::Null);
                }
                let mut last = None;
                for item in items {
                    if matches!(item, RhpType::Null) {
                        continue;
                    }
                    match self.coerce(item, val.clone(), param, class_name, span) {
                        Ok(produced) if self.value_matches(&produced, item) => return Ok(produced),
                        Ok(_) => {}
                        Err(err) => last = Some(err),
                    }
                }
                Err(last.unwrap_or_else(|| {
                    self.ser_err(
                        span,
                        format!(
                            "Cannot cast {} to {ty} for parameter ${param} of {class_name}",
                            val.type_name()
                        ),
                    )
                }))
            }
            RhpType::Null => {
                if val.is_null() {
                    Ok(PhpValue::Null)
                } else {
                    self.cast_mismatch(&val, ty, param, class_name, span)
                }
            }
            RhpType::Bool => coerce_bool(val).ok_or_else(|| {
                self.ser_err(
                    span,
                    format!("Cannot cast value to bool for parameter ${param} of {class_name}"),
                )
            }),
            RhpType::Int => coerce_int(val).ok_or_else(|| {
                self.ser_err(
                    span,
                    format!("Cannot cast value to int for parameter ${param} of {class_name}"),
                )
            }),
            RhpType::Float => coerce_float(val).ok_or_else(|| {
                self.ser_err(
                    span,
                    format!("Cannot cast value to float for parameter ${param} of {class_name}"),
                )
            }),
            RhpType::String => coerce_string(&val).map(PhpValue::string).ok_or_else(|| {
                self.ser_err(
                    span,
                    format!("Cannot cast {} to string for parameter ${param} of {class_name}", val.type_name()),
                )
            }),
            RhpType::Bytes => match val {
                PhpValue::Bytes(_) => Ok(val),
                PhpValue::String(s) => Ok(PhpValue::bytes(s.as_bytes().to_vec())),
                other => self.cast_mismatch(&other, ty, param, class_name, span),
            },
            RhpType::Rune => match val {
                PhpValue::Rune(_) => Ok(val),
                PhpValue::String(s) => {
                    let mut chars = s.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => Ok(PhpValue::Rune(c)),
                        _ => self.cast_mismatch(&PhpValue::String(s), ty, param, class_name, span),
                    }
                }
                other => self.cast_mismatch(&other, ty, param, class_name, span),
            },
            RhpType::Collection(CollectionSubtype::List(inner)) => {
                let items = expect_list(val).map_err(|_| {
                    self.ser_err(
                        span,
                        format!("Cannot cast value to list for parameter ${param} of {class_name}"),
                    )
                })?;
                if matches!(inner.as_ref(), RhpType::Mixed | RhpType::Dynamic) {
                    return Ok(PhpValue::list(items));
                }
                let mut out = Vec::with_capacity(items.len());
                for (idx, item) in items.into_iter().enumerate() {
                    out.push(self.cast_param(inner, item, &format!("{param}[{idx}]"), class_name, span)?);
                }
                Ok(PhpValue::list(out))
            }
            RhpType::Collection(CollectionSubtype::Dict(inner)) => {
                let entries = expect_dict(val).map_err(|_| {
                    self.ser_err(
                        span,
                        format!("Cannot cast value to dict for parameter ${param} of {class_name}"),
                    )
                })?;
                if matches!(inner.as_ref(), RhpType::Mixed | RhpType::Dynamic) {
                    return Ok(PhpValue::dict(entries));
                }
                let mut out = Vec::with_capacity(entries.len());
                for (key, item) in entries {
                    out.push((
                        key.clone(),
                        self.cast_param(inner, item, &format!("{param}.{key}"), class_name, span)?,
                    ));
                }
                Ok(PhpValue::dict(out))
            }
            RhpType::Array(_) | RhpType::Iterable(_) => match val {
                PhpValue::EmptyArray
                | PhpValue::Array(_)
                | PhpValue::List(_)
                | PhpValue::Dict(_)
                | PhpValue::Set(_)
                | PhpValue::HashMap(_) => Ok(val),
                other => self.cast_mismatch(&other, ty, param, class_name, span),
            },
            RhpType::Class(expected) => match &val {
                PhpValue::Object(rc) => {
                    let actual = rc.borrow().class_name.clone();
                    if self.is_class_instance_of(&actual, expected) {
                        Ok(val)
                    } else {
                        Err(self.ser_err(
                            span,
                            format!(
                                "Cannot cast {actual} to {expected} for parameter ${param} of {class_name}"
                            ),
                        ))
                    }
                }
                PhpValue::SharedObject(obj) => {
                    if self.is_class_instance_of(&obj.class_name, expected) {
                        Ok(val)
                    } else {
                        Err(self.ser_err(
                            span,
                            format!(
                                "Cannot cast {} to {expected} for parameter ${param} of {class_name}",
                                obj.class_name
                            ),
                        ))
                    }
                }
                other => self.cast_mismatch(other, ty, param, class_name, span),
            },
            RhpType::Object => match val {
                PhpValue::Object(_) | PhpValue::SharedObject(_) => Ok(val),
                other => self.cast_mismatch(&other, ty, param, class_name, span),
            },
            RhpType::Tuple(elems) => {
                let items = expect_list(val).map_err(|_| {
                    self.ser_err(
                        span,
                        format!("Cannot cast value to tuple for parameter ${param} of {class_name}"),
                    )
                })?;
                if items.len() != elems.len() {
                    return Err(self.ser_err(
                        span,
                        format!(
                            "Cannot cast list of length {} to tuple of length {} for parameter ${param} of {class_name}",
                            items.len(),
                            elems.len()
                        ),
                    ));
                }
                let mut out = Vec::with_capacity(items.len());
                for (idx, (item, elem_ty)) in items.into_iter().zip(elems.iter()).enumerate() {
                    out.push(self.cast_param(elem_ty, item, &format!("{param}[{idx}]"), class_name, span)?);
                }
                Ok(PhpValue::list(out))
            }
            other => {
                if self.value_matches(&val, other) {
                    Ok(val)
                } else {
                    self.cast_mismatch(&val, other, param, class_name, span)
                }
            }
        }
    }

    fn value_matches(&self, val: &PhpValue, ty: &RhpType) -> bool {
        match ty {
            RhpType::Class(expected) => match val {
                PhpValue::Object(rc) => {
                    let actual = rc.borrow().class_name.clone();
                    self.is_class_instance_of(&actual, expected)
                }
                PhpValue::SharedObject(obj) => self.is_class_instance_of(&obj.class_name, expected),
                _ => false,
            },
            RhpType::Nullable(inner) => val.is_null() || self.value_matches(val, inner),
            RhpType::Union(items) => items.iter().any(|item| self.value_matches(val, item)),
            _ => val.matches_type_contract(ty, self.type_hierarchy.as_ref()),
        }
    }

    fn cast_mismatch(
        &self,
        val: &PhpValue,
        ty: &RhpType,
        param: &str,
        class_name: &str,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        Err(self.ser_err(
            span,
            format!(
                "Cannot cast {} to {ty} for parameter ${param} of {class_name}",
                val.type_name()
            ),
        ))
    }

    fn ser_err(&self, span: Span, message: impl Into<String>) -> RuntimeError {
        self.make_error(
            RuntimeErrorKind::Custom {
                message: message.into(),
            },
            span,
        )
    }
}

fn bare_name(name: &str) -> &str {
    name.strip_prefix('$').unwrap_or(name)
}

fn type_accepts_null(ty: &RhpType) -> bool {
    match ty {
        RhpType::Null | RhpType::Mixed | RhpType::Dynamic | RhpType::Nullable(_) => true,
        RhpType::Union(items) => items.iter().any(type_accepts_null),
        _ => false,
    }
}

fn date_iso(obj: &VmObject) -> Option<String> {
    if !is_date_class(&obj.class_name) {
        return None;
    }
    if let Some(iso) = obj.get_property("iso") {
        let text = iso.to_php_string();
        if !text.is_empty() {
            return Some(to_wire_iso(&text));
        }
    }
    let ts = obj.get_property("timestamp").and_then(|v| v.to_i64())?;
    let micro = obj
        .get_property("micro")
        .and_then(|v| v.to_i64())
        .unwrap_or(0)
        .max(0) as u32;
    let tz = obj.get_property("timezone").map(|v| v.to_php_string());
    let date = crate::vm::time::TimeDate::create_from_timestamp(ts, micro, tz.as_deref());
    Some(to_wire_iso(&date.to_iso_string()))
}

fn is_date_class(name: &str) -> bool {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
    matches!(
        base,
        "Time" | "Carbon" | "DateTime" | "DateTimeImmutable" | "DateTimeInterface" | "Date"
    )
}

fn to_wire_iso(iso: &str) -> String {
    if let Some(rest) = iso.strip_suffix("+00:00") {
        format!("{rest}Z")
    } else {
        iso.to_string()
    }
}

fn json_number(n: &Number) -> PhpValue {
    if let Some(i) = n.as_i64() {
        PhpValue::Int(i)
    } else if let Some(u) = n.as_u64() {
        if let Ok(i) = i64::try_from(u) {
            PhpValue::Int(i)
        } else {
            PhpValue::float(u as f64)
        }
    } else {
        PhpValue::float(n.as_f64().unwrap_or(f64::NAN))
    }
}

fn coerce_bool(val: PhpValue) -> Option<PhpValue> {
    match val {
        PhpValue::Bool(b) => Some(PhpValue::Bool(b)),
        PhpValue::Int(0) => Some(PhpValue::Bool(false)),
        PhpValue::Int(1) => Some(PhpValue::Bool(true)),
        PhpValue::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(PhpValue::Bool(true)),
            "false" | "0" => Some(PhpValue::Bool(false)),
            _ => None,
        },
        _ => None,
    }
}

fn coerce_int(val: PhpValue) -> Option<PhpValue> {
    match val {
        PhpValue::Int(n) => Some(PhpValue::Int(n)),
        PhpValue::Float(f) => float_to_exact_i64(f).map(PhpValue::Int),
        PhpValue::String(s) => s.trim().parse::<i64>().ok().map(PhpValue::Int),
        PhpValue::Bool(b) => Some(PhpValue::Int(if b { 1 } else { 0 })),
        _ => None,
    }
}

fn float_to_exact_i64(f: f64) -> Option<i64> {
    if !f.is_finite() || f.fract() != 0.0 || f.abs() > (1i64 << 53) as f64 {
        return None;
    }
    Some(f as i64)
}

fn coerce_float(val: PhpValue) -> Option<PhpValue> {
    match val {
        PhpValue::Float(f) if f.is_finite() => Some(PhpValue::float(f)),
        PhpValue::Int(n) => Some(PhpValue::float(n as f64)),
        PhpValue::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(PhpValue::float),
        PhpValue::Bool(b) => Some(PhpValue::float(if b { 1.0 } else { 0.0 })),
        _ => None,
    }
}

fn coerce_string(val: &PhpValue) -> Option<String> {
    match val {
        PhpValue::String(s) => Some(s.to_string()),
        PhpValue::Int(n) => Some(n.to_string()),
        PhpValue::Float(f) if f.is_finite() => Some(f.to_string()),
        PhpValue::Bool(true) => Some("true".to_string()),
        PhpValue::Bool(false) => Some("false".to_string()),
        PhpValue::Rune(c) => Some(c.to_string()),
        _ => None,
    }
}

fn expect_list(val: PhpValue) -> Result<Vec<PhpValue>, ()> {
    match val {
        PhpValue::EmptyArray => Ok(Vec::new()),
        PhpValue::List(items) | PhpValue::Array(items) => Ok(items.borrow().clone()),
        _ => Err(()),
    }
}

fn expect_dict(val: PhpValue) -> Result<Vec<(String, PhpValue)>, ()> {
    match val {
        PhpValue::EmptyArray => Ok(Vec::new()),
        PhpValue::Dict(map) => Ok(map.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        _ => Err(()),
    }
}

/// Ordered JSON so constructor fields stay in declaration order.
enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn write(&self) -> String {
        let mut out = String::new();
        self.push(&mut out);
        out
    }

    fn push(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Float(f) => {
                if let Some(n) = Number::from_f64(*f) {
                    out.push_str(&n.to_string());
                }
            }
            Json::String(s) => {
                out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()));
            }
            Json::Array(items) => {
                out.push('[');
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        out.push(',');
                    }
                    item.push(out);
                }
                out.push(']');
            }
            Json::Object(entries) => {
                out.push('{');
                for (idx, (key, val)) in entries.iter().enumerate() {
                    if idx > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()));
                    out.push(':');
                    val.push(out);
                }
                out.push('}');
            }
        }
    }
}

fn arg_value(value: PhpValue) -> StagedArg {
    StagedArg {
        value,
        name: None,
        is_unpack: false,
        is_reference: false,
        is_skipped: false,
        caller_reg: None,
    }
}

fn arg_skip() -> StagedArg {
    StagedArg {
        value: PhpValue::Null,
        name: None,
        is_unpack: false,
        is_reference: false,
        is_skipped: true,
        caller_reg: None,
    }
}

fn arg_unpack(value: PhpValue) -> StagedArg {
    StagedArg {
        value,
        name: None,
        is_unpack: true,
        is_reference: false,
        is_skipped: false,
        caller_reg: None,
    }
}

fn write_props(instance: &PhpValue, pairs: &[(String, PhpValue)]) {
    if let PhpValue::Object(rc) = instance {
        let mut obj = rc.borrow_mut();
        for (name, val) in pairs {
            obj.set_property(name.clone(), val.clone());
        }
    }
}
