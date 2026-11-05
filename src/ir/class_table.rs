//! # RHP Runtime Class Table & Linker
//!
//! Implements flat vtables, slot-indexed property layouts, and ancestor sets
//! resolved at link time for classes, abstract classes, interfaces, and traits.
//! Single inheritance with PHP-compliant trait precedence (class > trait > parent).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::ast::types::Visibility;
use crate::ir::error::{IrError, IrErrorKind};
use crate::ir::opcode::{BytecodeChunk, ClassId, FunctionId};
use crate::ir::{CompiledFunction, CompiledProperty};
use crate::lexer::span::Span;
use crate::type_system::types::RhpType;

pub type MethodId = FunctionId;

/// Single property slot in an object instance layout (flattened, slot-indexed).
#[derive(Debug, Clone, PartialEq)]
pub struct PropSlot {
    pub name: Arc<str>,
    pub rhp_type: RhpType,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_readonly: bool,
    pub slot_index: usize,
    pub declaring_class: ClassId,
    pub default_chunk: Option<BytecodeChunk>,
}

/// Runtime class descriptor in the global class table.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    pub id: ClassId,
    pub name: Arc<str>,
    pub parent: Option<ClassId>,
    pub parent_name: Option<String>,
    pub interfaces: Vec<ClassId>,
    pub interface_names: Vec<String>,
    pub traits: Vec<ClassId>,
    pub trait_names: Vec<String>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    pub is_shared: bool,
    pub is_static_only: bool,
    pub is_open: bool,
    pub is_sealed: bool,
    pub is_trait: bool,
    pub is_interface: bool,

    // Own declarations, as written in source
    pub own_methods: HashMap<Arc<str>, MethodId>,
    pub own_props: Vec<CompiledProperty>,

    // Resolved at link time (flattened)
    pub vtable: HashMap<Arc<str>, MethodId>,   // name -> final method incl. inherited and traits
    pub static_forward: Option<MethodId>,       // reserved class static forwarder (__forward)
    pub call_slot: Option<MethodId>,            // reserved instance dynamic forwarder (__call)
    pub call_static_slot: Option<MethodId>,     // reserved static dynamic forwarder (__callStatic)
    pub constructor_slot: Option<MethodId>,     // resolved constructor (__construct)
    pub is_escaped: bool,                       // marked true when class escapes compile-time known direct calls (e.g. ::class)
    pub method_slots: Vec<MethodId>,            // dense slot-indexed vtable for O(1) array dispatch
    pub method_indices: HashMap<Arc<str>, usize>, // method name to vtable slot index
    pub final_methods: HashSet<Arc<str>>,       // methods declared or inherited as final (cannot be overridden)
    pub prop_layout: Vec<PropSlot>,            // parent props first, then own
    pub prop_indices: HashMap<Arc<str>, usize>,
    pub ancestors: HashSet<ClassId>,           // self + parents + interfaces + traits
    pub ancestor_names: HashSet<String>,
    pub span: Span,
}

impl ClassInfo {
    pub fn new(id: ClassId, name: impl Into<Arc<str>>, span: Span) -> Self {
        let n = name.into();
        Self {
            id,
            name: n,
            parent: None,
            parent_name: None,
            interfaces: Vec::new(),
            interface_names: Vec::new(),
            traits: Vec::new(),
            trait_names: Vec::new(),
            is_abstract: false,
            is_final: false,
            is_readonly: false,
            is_shared: false,
            is_static_only: false,
            is_open: false,
            is_sealed: false,
            is_trait: false,
            is_interface: false,
            own_methods: HashMap::new(),
            own_props: Vec::new(),
            vtable: HashMap::new(),
            static_forward: None,
            call_slot: None,
            call_static_slot: None,
            constructor_slot: None,
            is_escaped: false,
            method_slots: Vec::new(),
            method_indices: HashMap::new(),
            final_methods: HashSet::new(),
            prop_layout: Vec::new(),
            prop_indices: HashMap::new(),
            ancestors: HashSet::new(),
            ancestor_names: HashSet::new(),
            span,
        }
    }

    /// Retrieve the declaring [`ClassId`] of a given linked method.
    #[inline]
    pub fn decl_of(&self, method_id: MethodId, functions: &[CompiledFunction]) -> Option<ClassId> {
        functions.get(method_id.index()).and_then(|f| f.declaring_class_id)
    }
}

/// Global Class Table holding all compiled and linked classes, interfaces, and traits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassTable {
    pub classes: Vec<ClassInfo>,
    pub name_to_id: HashMap<String, ClassId>,
}

impl ClassTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: ClassId) -> Option<&ClassInfo> {
        self.classes.get(id.index())
    }

    pub fn get_mut(&mut self, id: ClassId) -> Option<&mut ClassInfo> {
        self.classes.get_mut(id.index())
    }

    pub fn get_by_name(&self, name: &str) -> Option<&ClassInfo> {
        let clean = name.strip_prefix('\\').unwrap_or(name);
        if let Some(&id) = self.name_to_id.get(clean) {
            return self.get(id);
        }
        if let Some(&id) = self.name_to_id.get(name) {
            return self.get(id);
        }
        self.classes.iter().find(|c| c.name.as_ref() == clean || c.name.as_ref() == name)
    }

    pub fn add_class(&mut self, mut info: ClassInfo) -> ClassId {
        let id = ClassId(self.classes.len() as u32);
        info.id = id;
        let name_str = info.name.to_string();
        let clean_name = name_str.strip_prefix('\\').unwrap_or(&name_str).to_string();
        self.name_to_id.insert(name_str.clone(), id);
        if clean_name != name_str {
            self.name_to_id.insert(clean_name, id);
        }
        self.classes.push(info);
        id
    }

    /// Link all registered classes, traits, and interfaces.
    /// Flattens vtables (class > trait > parent), prop layouts, and builds ancestor sets.
    pub fn link(
        &mut self,
        functions: &mut Vec<CompiledFunction>,
        function_index: &mut HashMap<String, FunctionId>,
    ) -> Result<(), IrError> {
        // 1. Resolve parent, interface, and trait IDs from string names
        let num_entries = self.classes.len();
        for i in 0..num_entries {
            let parent_opt = self.classes[i].parent_name.clone();
            if let Some(ref p_name) = parent_opt {
                let clean_p = p_name.strip_prefix('\\').unwrap_or(p_name);
                if let Some(&pid) = self.name_to_id.get(clean_p).or_else(|| self.name_to_id.get(p_name)) {
                    self.classes[i].parent = Some(pid);
                }
            }

            let ifaces = self.classes[i].interface_names.clone();
            for iface_name in ifaces {
                let clean_i = iface_name.strip_prefix('\\').unwrap_or(&iface_name);
                if let Some(&iid) = self.name_to_id.get(clean_i).or_else(|| self.name_to_id.get(&iface_name)) {
                    if !self.classes[i].interfaces.contains(&iid) {
                        self.classes[i].interfaces.push(iid);
                    }
                }
            }

            let trs = self.classes[i].trait_names.clone();
            for tr_name in trs {
                let clean_t = tr_name.strip_prefix('\\').unwrap_or(&tr_name);
                if let Some(&tid) = self.name_to_id.get(clean_t).or_else(|| self.name_to_id.get(&tr_name)) {
                    if !self.classes[i].traits.contains(&tid) {
                        self.classes[i].traits.push(tid);
                    }
                }
            }
        }

        // 2. Topological sort of classes by parent to detect inheritance cycles
        let topo_order = self.compute_topological_order()?;

        // 3. Link interfaces first (interface inheritance: extends)
        for i in 0..num_entries {
            if self.classes[i].is_interface {
                self.link_interface(ClassId(i as u32))?;
            }
        }

        // 4. Link traits (traits can use other traits)
        for i in 0..num_entries {
            if self.classes[i].is_trait {
                self.link_trait(ClassId(i as u32), functions, function_index)?;
            }
        }

        // 5. Link classes in topological order
        for cid in topo_order {
            if !self.classes[cid.index()].is_interface && !self.classes[cid.index()].is_trait {
                self.link_class(cid, functions, function_index)?;
            }
        }

        Ok(())
    }

    fn compute_topological_order(&self) -> Result<Vec<ClassId>, IrError> {
        let mut visited = vec![0u8; self.classes.len()]; // 0: unvisited, 1: visiting, 2: visited
        let mut order = Vec::with_capacity(self.classes.len());

        for i in 0..self.classes.len() {
            if visited[i] == 0 && !self.classes[i].is_interface && !self.classes[i].is_trait {
                self.dfs_topo(ClassId(i as u32), &mut visited, &mut order)?;
            }
        }

        Ok(order)
    }

    fn dfs_topo(
        &self,
        id: ClassId,
        visited: &mut [u8],
        order: &mut Vec<ClassId>,
    ) -> Result<(), IrError> {
        let idx = id.index();
        if visited[idx] == 1 {
            return Err(IrError::new(
                IrErrorKind::InheritanceCycle {
                    class_name: self.classes[idx].name.to_string(),
                },
                self.classes[idx].span,
            ));
        }
        if visited[idx] == 2 {
            return Ok(());
        }

        visited[idx] = 1;

        if let Some(parent_id) = self.classes[idx].parent {
            if parent_id.index() < self.classes.len() {
                self.dfs_topo(parent_id, visited, order)?;
            }
        }

        visited[idx] = 2;
        order.push(id);
        Ok(())
    }

    fn link_interface(&mut self, id: ClassId) -> Result<(), IrError> {
        let idx = id.index();
        let mut ancestors = HashSet::new();
        let mut ancestor_names = HashSet::new();

        ancestors.insert(id);
        ancestor_names.insert(self.classes[idx].name.to_string());
        let clean = self.classes[idx].name.strip_prefix('\\').unwrap_or(&self.classes[idx].name).to_string();
        ancestor_names.insert(clean);

        // Inherit extended interface ancestors and methods
        let ifaces = self.classes[idx].interfaces.clone();
        let mut vtable = self.classes[idx].own_methods.clone();

        for parent_iface_id in ifaces {
            let p_idx = parent_iface_id.index();
            if p_idx < self.classes.len() {
                ancestors.insert(parent_iface_id);
                ancestors.extend(self.classes[p_idx].ancestors.iter().copied());
                ancestor_names.extend(self.classes[p_idx].ancestor_names.iter().cloned());
                for (name, &mid) in &self.classes[p_idx].vtable {
                    vtable.entry(name.clone()).or_insert(mid);
                }
            }
        }

        self.classes[idx].ancestors = ancestors;
        self.classes[idx].ancestor_names = ancestor_names;
        self.classes[idx].call_slot = vtable.get("__call").copied();
        self.classes[idx].call_static_slot = vtable.get("__callStatic").copied();
        self.classes[idx].constructor_slot = vtable.get("__construct").copied();
        let mut method_slots = Vec::with_capacity(vtable.len());
        let mut method_indices = HashMap::with_capacity(vtable.len());
        for (m_name, &mid) in &vtable {
            let slot = method_slots.len();
            method_slots.push(mid);
            method_indices.insert(m_name.clone(), slot);
        }
        self.classes[idx].method_slots = method_slots;
        self.classes[idx].method_indices = method_indices;
        self.classes[idx].vtable = vtable;
        Ok(())
    }

    fn link_trait(
        &mut self,
        id: ClassId,
        _functions: &mut Vec<CompiledFunction>,
        _function_index: &mut HashMap<String, FunctionId>,
    ) -> Result<(), IrError> {
        let idx = id.index();
        let mut vtable = self.classes[idx].own_methods.clone();
        let mut ancestors = HashSet::new();
        let mut ancestor_names = HashSet::new();

        ancestors.insert(id);
        ancestor_names.insert(self.classes[idx].name.to_string());
        let clean = self.classes[idx].name.strip_prefix('\\').unwrap_or(&self.classes[idx].name).to_string();
        ancestor_names.insert(clean);

        // Nested traits
        let used_traits = self.classes[idx].traits.clone();
        for tr_id in used_traits {
            let tr_idx = tr_id.index();
            if tr_idx < self.classes.len() {
                ancestors.insert(tr_id);
                ancestors.extend(self.classes[tr_idx].ancestors.iter().copied());
                ancestor_names.extend(self.classes[tr_idx].ancestor_names.iter().cloned());

                for (name, &mid) in &self.classes[tr_idx].vtable {
                    vtable.entry(name.clone()).or_insert(mid);
                }
            }
        }

        self.classes[idx].ancestors = ancestors;
        self.classes[idx].ancestor_names = ancestor_names;
        if let Some(&mid) = vtable.get("__forward") {
            self.classes[idx].static_forward = Some(mid);
        }
        self.classes[idx].call_slot = vtable.get("__call").copied();
        self.classes[idx].call_static_slot = vtable.get("__callStatic").copied();
        self.classes[idx].constructor_slot = vtable.get("__construct").copied();
        let mut method_slots = Vec::with_capacity(vtable.len());
        let mut method_indices = HashMap::with_capacity(vtable.len());
        for (m_name, &mid) in &vtable {
            let slot = method_slots.len();
            method_slots.push(mid);
            method_indices.insert(m_name.clone(), slot);
        }
        self.classes[idx].method_slots = method_slots;
        self.classes[idx].method_indices = method_indices;
        self.classes[idx].vtable = vtable;
        Ok(())
    }

    fn link_class(
        &mut self,
        id: ClassId,
        functions: &mut Vec<CompiledFunction>,
        function_index: &mut HashMap<String, FunctionId>,
    ) -> Result<(), IrError> {
        let idx = id.index();
        let class_name = self.classes[idx].name.clone();
        let span = self.classes[idx].span;
        let is_abstract = self.classes[idx].is_abstract;

        let mut vtable = HashMap::new();
        let mut static_forward = None;
        let mut final_methods = HashSet::new();
        let mut prop_layout = Vec::new();
        let mut prop_indices = HashMap::new();
        let mut ancestors = HashSet::new();
        let mut ancestor_names = HashSet::new();

        ancestors.insert(id);
        ancestor_names.insert(class_name.to_string());
        let clean_self = class_name.strip_prefix('\\').unwrap_or(&class_name).to_string();
        ancestor_names.insert(clean_self);

        // 1. Start with parent's vtable, prop_layout, ancestors, and static_forward
        if let Some(parent_id) = self.classes[idx].parent {
            let p_idx = parent_id.index();
            if p_idx < self.classes.len() {
                if self.classes[p_idx].is_final {
                    return Err(IrError::new(
                        IrErrorKind::CannotExtendFinalClass {
                            class_name: class_name.to_string(),
                            parent_name: self.classes[p_idx].name.to_string(),
                        },
                        span,
                    ));
                }

                if self.classes[p_idx].is_sealed {
                    return Err(IrError::new(
                        IrErrorKind::CannotExtendSealedClass {
                            class_name: class_name.to_string(),
                            parent_name: self.classes[p_idx].name.to_string(),
                        },
                        span,
                    ));
                }

                vtable = self.classes[p_idx].vtable.clone();
                static_forward = self.classes[p_idx].static_forward;
                final_methods = self.classes[p_idx].final_methods.clone();
                prop_layout = self.classes[p_idx].prop_layout.clone();
                prop_indices = self.classes[p_idx].prop_indices.clone();

                ancestors.insert(parent_id);
                ancestors.extend(self.classes[p_idx].ancestors.iter().copied());
                ancestor_names.extend(self.classes[p_idx].ancestor_names.iter().cloned());
            }
        } else if let Some(ref p_name) = self.classes[idx].parent_name {
            // Built-in parent (e.g. Exception, stdClass)
            let clean_p = p_name.strip_prefix('\\').unwrap_or(p_name);
            ancestor_names.insert(p_name.clone());
            ancestor_names.insert(clean_p.to_string());
            if clean_p.ends_with("Exception") || clean_p.ends_with("Error") || clean_p == "Throwable" {
                ancestor_names.insert("Throwable".to_string());
                ancestor_names.insert("Exception".to_string());
            }
        }

        // 2. Apply each used Trait: Trait methods override inherited parent methods
        let used_traits = self.classes[idx].traits.clone();
        let mut trait_method_providers: HashMap<Arc<str>, ClassId> = HashMap::new();
        for tr_id in &used_traits {
            let tr_idx = tr_id.index();
            if tr_idx < self.classes.len() {
                ancestors.insert(*tr_id);
                ancestors.extend(self.classes[tr_idx].ancestors.iter().copied());
                ancestor_names.extend(self.classes[tr_idx].ancestor_names.iter().cloned());

                // Copy trait properties
                for prop in &self.classes[tr_idx].own_props {
                    if !prop.is_static {
                        let prop_name: Arc<str> = prop.name.clone().into();
                        if !prop_indices.contains_key(&prop_name) {
                            let slot_index = prop_layout.len();
                            prop_indices.insert(prop_name.clone(), slot_index);
                            prop_layout.push(PropSlot {
                                name: prop_name,
                                rhp_type: prop.rhp_type.clone(),
                                visibility: prop.visibility,
                                is_static: false,
                                is_readonly: prop.is_readonly,
                                slot_index,
                                declaring_class: id,
                                default_chunk: prop.default_chunk.clone(),
                            });
                        }
                    }
                }

                // Trait methods: clone with declaring_class = using class
                let tr_methods = self.classes[tr_idx].vtable.clone();
                for (m_name, tr_mid) in tr_methods {
                    // If class already declares its own method, own method will override in step 3
                    if self.classes[idx].own_methods.contains_key(&m_name) {
                        continue;
                    }

                    // Enforce explicit trait composition: conflicting trait methods without class resolution are errors
                    if let Some(&first_tr_id) = trait_method_providers.get(&m_name) {
                        if first_tr_id != *tr_id {
                            return Err(IrError::new(
                                IrErrorKind::TraitMethodCollision {
                                    class_name: class_name.to_string(),
                                    method_name: m_name.to_string(),
                                    trait_a: self.classes[first_tr_id.index()].name.to_string(),
                                    trait_b: self.classes[tr_idx].name.to_string(),
                                },
                                span,
                            ));
                        }
                    }
                    trait_method_providers.insert(m_name.clone(), *tr_id);

                    let orig_func = functions[tr_mid.index()].clone();
                    let new_id = FunctionId(functions.len() as u32);
                    let mut cloned_func = orig_func;
                    cloned_func.id = new_id;
                    let method_key = format!("{}::{}", class_name, m_name);
                    cloned_func.name = method_key.clone();
                    cloned_func.enclosing_class = Some(class_name.to_string());
                    cloned_func.declaring_class_id = Some(id);

                    functions.push(cloned_func);
                    function_index.insert(method_key, new_id);
                    vtable.insert(m_name, new_id);
                }
            }
        }

        // 3. Apply class's own methods: they override trait methods and parent methods
        let own_methods = self.classes[idx].own_methods.clone();
        for (m_name, mid) in own_methods {
            let child_func = &functions[mid.index()];
            let has_inherited = vtable.contains_key(&m_name);

            if child_func.is_override && !has_inherited {
                return Err(IrError::new(
                    IrErrorKind::MethodDoesNotOverride {
                        class_name: class_name.to_string(),
                        method_name: m_name.to_string(),
                    },
                    span,
                ));
            }

            if let Some(&parent_mid) = vtable.get(&m_name) {
                if mid != parent_mid {
                    // Verify override compatibility
                    let parent_func = &functions[parent_mid.index()];
                    let child_func = &functions[mid.index()];

                    if parent_func.is_final {
                        return Err(IrError::new(
                            IrErrorKind::CannotOverrideFinalMethod {
                                class_name: class_name.to_string(),
                                parent_name: parent_func.enclosing_class.clone().unwrap_or_default(),
                                method_name: m_name.to_string(),
                            },
                            span,
                        ));
                    }

                    // Visibility narrowing check
                    if !Self::is_visibility_compatible(child_func.visibility, parent_func.visibility) {
                        return Err(IrError::new(
                            IrErrorKind::NarrowedVisibility {
                                class_name: class_name.to_string(),
                                parent_name: parent_func.enclosing_class.clone().unwrap_or_default(),
                                method_name: m_name.to_string(),
                                parent_visibility: format!("{:?}", parent_func.visibility).to_lowercase(),
                            },
                            span,
                        ));
                    }
                }
            }

            if functions[mid.index()].is_final {
                final_methods.insert(m_name.clone());
            }

            functions[mid.index()].declaring_class_id = Some(id);
            vtable.insert(m_name, mid);
        }

        // 4. Append own instance properties
        for prop in &self.classes[idx].own_props {
            if !prop.is_static {
                let prop_name: Arc<str> = prop.name.clone().into();
                if let Some(&existing_slot) = prop_indices.get(&prop_name) {
                    // Update slot metadata for overridden property
                    prop_layout[existing_slot].rhp_type = prop.rhp_type.clone();
                    prop_layout[existing_slot].visibility = prop.visibility;
                    prop_layout[existing_slot].is_readonly = prop.is_readonly;
                    prop_layout[existing_slot].declaring_class = id;
                    if prop.default_chunk.is_some() {
                        prop_layout[existing_slot].default_chunk = prop.default_chunk.clone();
                    }
                } else {
                    let slot_index = prop_layout.len();
                    prop_indices.insert(prop_name.clone(), slot_index);
                    prop_layout.push(PropSlot {
                        name: prop_name,
                        rhp_type: prop.rhp_type.clone(),
                        visibility: prop.visibility,
                        is_static: false,
                        is_readonly: prop.is_readonly,
                        slot_index,
                        declaring_class: id,
                        default_chunk: prop.default_chunk.clone(),
                    });
                }
            }
        }

        // 5. Interfaces validation
        let ifaces = self.classes[idx].interfaces.clone();
        for iface_id in ifaces {
            let i_idx = iface_id.index();
            if i_idx < self.classes.len() {
                ancestors.insert(iface_id);
                ancestors.extend(self.classes[i_idx].ancestors.iter().copied());
                ancestor_names.extend(self.classes[i_idx].ancestor_names.iter().cloned());

                // Check that all interface methods are implemented
                for (iface_m_name, &_iface_mid) in &self.classes[i_idx].vtable {
                    let impl_mid_opt = vtable.get(iface_m_name).copied();
                    match impl_mid_opt {
                        None => {
                            return Err(IrError::new(
                                IrErrorKind::InterfaceNotImplemented {
                                    class_name: class_name.to_string(),
                                    interface_name: self.classes[i_idx].name.to_string(),
                                    method_name: iface_m_name.to_string(),
                                },
                                span,
                            ));
                        }
                        Some(impl_mid) => {
                            let impl_func = &functions[impl_mid.index()];
                            if impl_func.visibility != Visibility::Public {
                                return Err(IrError::new(
                                    IrErrorKind::NarrowedVisibility {
                                        class_name: class_name.to_string(),
                                        parent_name: self.classes[i_idx].name.to_string(),
                                        method_name: iface_m_name.to_string(),
                                        parent_visibility: "public".to_string(),
                                    },
                                    span,
                                ));
                            }
                            if !is_abstract && impl_func.is_abstract {
                                return Err(IrError::new(
                                    IrErrorKind::InterfaceNotImplemented {
                                        class_name: class_name.to_string(),
                                        interface_name: self.classes[i_idx].name.to_string(),
                                        method_name: iface_m_name.to_string(),
                                    },
                                    span,
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 6. Check that trait abstract methods are satisfied if class is concrete
        if !is_abstract {
            for tr_id in &used_traits {
                let tr_idx = tr_id.index();
                if tr_idx < self.classes.len() {
                    for (m_name, &tr_mid) in &self.classes[tr_idx].own_methods {
                        if functions[tr_mid.index()].is_abstract {
                            let impl_func = vtable.get(m_name).map(|&mid| &functions[mid.index()]);
                            if impl_func.map(|f| f.is_abstract).unwrap_or(true) {
                                return Err(IrError::new(
                                    IrErrorKind::TraitAbstractMethodUnimplemented {
                                        class_name: class_name.to_string(),
                                        trait_name: self.classes[tr_idx].name.to_string(),
                                        method_name: m_name.to_string(),
                                    },
                                    span,
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 7. Verify no abstract methods remain unless class is abstract
        if !is_abstract {
            for (m_name, &mid) in &vtable {
                let f = &functions[mid.index()];
                if f.is_abstract {
                    return Err(IrError::new(
                        IrErrorKind::UnimplementedAbstractMethod {
                            class_name: class_name.to_string(),
                            method_name: m_name.to_string(),
                            declaring_class: f.enclosing_class.clone().unwrap_or_default(),
                        },
                        span,
                    ));
                }
            }
        }

        // 8. Abstract method cannot be declared private
        for (m_name, &mid) in &self.classes[idx].own_methods {
            let f = &functions[mid.index()];
            if f.is_abstract && f.visibility == Visibility::Private {
                return Err(IrError::new(
                    IrErrorKind::AbstractMethodPrivate {
                        class_name: class_name.to_string(),
                        method_name: m_name.to_string(),
                    },
                    span,
                ));
            }
        }

        if let Some(&mid) = vtable.get("__forward") {
            static_forward = Some(mid);
        }
        self.classes[idx].static_forward = static_forward;
        self.classes[idx].call_slot = vtable.get("__call").copied();
        self.classes[idx].call_static_slot = vtable.get("__callStatic").copied();
        self.classes[idx].constructor_slot = vtable.get("__construct").copied();
        let mut method_slots = Vec::with_capacity(vtable.len());
        let mut method_indices = HashMap::with_capacity(vtable.len());
        for (m_name, &mid) in &vtable {
            let slot = method_slots.len();
            method_slots.push(mid);
            method_indices.insert(m_name.clone(), slot);
        }
        self.classes[idx].final_methods = final_methods;
        self.classes[idx].method_slots = method_slots;
        self.classes[idx].method_indices = method_indices;
        self.classes[idx].vtable = vtable;
        self.classes[idx].prop_layout = prop_layout;
        self.classes[idx].prop_indices = prop_indices;
        self.classes[idx].ancestors = ancestors;
        self.classes[idx].ancestor_names = ancestor_names;

        Ok(())
    }

    /// Mark a class as escaped (its name was referenced via `Foo::class`, dynamic container, or reflection).
    pub fn mark_escaped(&mut self, class_name: &str) {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        if let Some(&id) = self.name_to_id.get(clean).or_else(|| self.name_to_id.get(class_name)) {
            if let Some(ci) = self.classes.get_mut(id.index()) {
                ci.is_escaped = true;
            }
        }
    }

    /// Check whether a class is marked as escaped.
    pub fn is_class_escaped(&self, class_name: &str) -> bool {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        self.get_by_name(clean).map(|c| c.is_escaped).unwrap_or(false)
    }

    /// Whole-Program Class Hierarchy Analysis (CHA):
    /// Returns true if any subclass of `base_class` overrides `method_name`.
    /// If false, calls to `base_class->method_name` can be safely devirtualized to direct calls.
    pub fn is_method_overridden(&self, base_class: &str, method_name: &str) -> bool {
        let clean = base_class.strip_prefix('\\').unwrap_or(base_class);
        let base_id = match self.get_by_name(clean).map(|c| c.id) {
            Some(id) => id,
            None => return true, // conservative fallback if class is unknown
        };

        if self.classes[base_id.index()].is_final
            || self.classes[base_id.index()].is_sealed
            || self.classes[base_id.index()].final_methods.contains(method_name)
        {
            return false;
        }

        let base_mid = match self.classes[base_id.index()].vtable.get(method_name) {
            Some(&mid) => mid,
            None => return false,
        };

        for class in &self.classes {
            if class.id == base_id || class.is_interface || class.is_trait {
                continue;
            }
            if class.ancestors.contains(&base_id) {
                if let Some(&sub_mid) = class.vtable.get(method_name) {
                    if sub_mid != base_mid {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Visibility narrowing check: child visibility cannot be narrower than parent visibility.
    /// Public is widest, Private is narrowest.
    fn is_visibility_compatible(child: Visibility, parent: Visibility) -> bool {
        match parent {
            Visibility::Public => child == Visibility::Public,
            Visibility::Protected => child == Visibility::Public || child == Visibility::Protected,
            Visibility::Private => true, // private methods in parent can be declared with any visibility in child
        }
    }
}
