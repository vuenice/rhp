use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock, RwLock};

/// A thread-safe directive handler function or closure.
///
/// Handlers take optional directive arguments (`Option<&str>`) and return
/// the generated RHP code as a `String`.
pub type DirectiveHandler = Arc<dyn Fn(Option<&str>) -> String + Send + Sync + 'static>;

/// A thread-safe registry for user-defined custom directives.
///
/// Directives can be registered per compiler instance or globally via `DirectiveRegistry::global()`.
#[derive(Clone, Default)]
pub struct DirectiveRegistry {
    handlers: Arc<RwLock<HashMap<String, DirectiveHandler>>>,
}

impl DirectiveRegistry {
    /// Creates a new empty `DirectiveRegistry`.
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns a reference to the global `DirectiveRegistry` singleton.
    pub fn global() -> &'static DirectiveRegistry {
        static GLOBAL: OnceLock<DirectiveRegistry> = OnceLock::new();
        GLOBAL.get_or_init(DirectiveRegistry::new)
    }

    /// Registers a custom directive by name with a handler function or closure.
    ///
    /// The handler receives `args: Option<&str>` and returns `String` (the generated RHP code).
    pub fn register<F>(&self, name: impl Into<String>, handler: F) -> &Self
    where
        F: Fn(Option<&str>) -> String + Send + Sync + 'static,
    {
        if let Ok(mut guard) = self.handlers.write() {
            guard.insert(name.into(), Arc::new(handler));
        }
        self
    }

    /// Registers a custom directive by name using an existing `DirectiveHandler`.
    pub fn register_handler(&self, name: impl Into<String>, handler: DirectiveHandler) -> &Self {
        if let Ok(mut guard) = self.handlers.write() {
            guard.insert(name.into(), handler);
        }
        self
    }

    /// Retrieves the directive handler for the given directive name, if registered.
    pub fn get(&self, name: &str) -> Option<DirectiveHandler> {
        self.handlers.read().ok()?.get(name).cloned()
    }

    /// Returns `true` if a directive with the given name is registered in this registry.
    pub fn contains(&self, name: &str) -> bool {
        self.handlers
            .read()
            .map_or(false, |guard| guard.contains_key(name))
    }

    /// Alias for `contains`.
    pub fn has(&self, name: &str) -> bool {
        self.contains(name)
    }

    /// Invokes the handler for the given directive name, if registered.
    /// Returns `Some(String)` with the generated RHP code, or `None` if unregistered.
    pub fn call(&self, name: &str, args: Option<&str>) -> Option<String> {
        let handler = self.get(name)?;
        Some(handler(args))
    }

    /// Unregisters a directive by name. Returns `true` if it was registered.
    pub fn remove(&self, name: &str) -> bool {
        self.handlers
            .write()
            .map_or(false, |mut guard| guard.remove(name).is_some())
    }

    /// Removes all registered directives from this registry.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.handlers.write() {
            guard.clear();
        }
    }

    /// Returns the number of registered directives in this registry.
    pub fn len(&self) -> usize {
        self.handlers.read().map_or(0, |guard| guard.len())
    }

    /// Returns `true` if the registry contains no registered directives.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a list of all registered directive names.
    pub fn names(&self) -> Vec<String> {
        self.handlers
            .read()
            .map(|guard| guard.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Creates an isolated copy of this registry with independent storage.
    pub fn clone_isolated(&self) -> Self {
        let cloned_map = self
            .handlers
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        Self {
            handlers: Arc::new(RwLock::new(cloned_map)),
        }
    }
}

impl fmt::Debug for DirectiveRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys = self.names();
        f.debug_struct("DirectiveRegistry")
            .field("directives", &keys)
            .finish()
    }
}

/// Helper function to register a custom directive globally.
pub fn register_directive<F>(name: impl Into<String>, handler: F)
where
    F: Fn(Option<&str>) -> String + Send + Sync + 'static,
{
    DirectiveRegistry::global().register(name, handler);
}
