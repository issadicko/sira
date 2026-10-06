//! Réglages réseau d'une requête : redirections, et (à venir) TLS, proxy et cookies.

/// Les redirections, comme Bruno : 301, 302, 303, 307 et 308 sont suivies jusqu'à `max` fois ; la réponse qui dépasse la
/// limite est rendue telle quelle. `forward_authorization` vaut `true` à l'exécution quand le fichier ne dit rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redirects {
    pub follow: bool,
    pub max: u32,
    pub forward_authorization: bool,
}

impl Redirects {
    pub const DEFAULT_MAX: u32 = 5;

    /// Aucune redirection n'est suivie : la 3xx est la réponse.
    pub const fn none() -> Self {
        Self { follow: false, max: 0, forward_authorization: true }
    }

    pub const fn limit(&self) -> u32 {
        if self.follow {
            self.max
        } else {
            0
        }
    }
}

impl Default for Redirects {
    fn default() -> Self {
        Self { follow: true, max: Self::DEFAULT_MAX, forward_authorization: true }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Network {
    pub redirects: Redirects,
}
