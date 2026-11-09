//! Every Google account of GNOME Online Accounts (§10.1), on the bus the
//! process is given, never another: their discovery is her protocol's
//! crate's (`hocus_secretary_link::goa`, which the Settings use too);
//! their tokens are asked for here, each time ([`token`]), never kept.
//! Each consent sheet's words are `hocus_secretary_link::sheets`'.

use hocus_day::google::AccessToken;
use zbus::OwnedObjectPath;

pub use hocus_secretary_link::goa::{ACCOUNT, GoogleAccount, Imap, MAIL, NAME, OAUTH2, ROOT};

use crate::{Result, SourceError};

/// Every Google account Online Accounts has on `bus` (an address), in
/// its objects' order, each with a suggested notebook. Blocking, within
/// 30 s.
pub fn discover(bus: &str) -> Result<Vec<GoogleAccount>> {
    hocus_secretary_link::goa::discover(bus).map_err(SourceError::Offline)
}

/// The account at `path`'s access tokens, asked of Online Accounts on
/// `bus` each time one is needed (`hocus-day`'s).
pub fn token(bus: &str, path: &str) -> Result<Box<dyn AccessToken>> {
    let path = OwnedObjectPath::try_from(path)
        .map_err(|err| SourceError::Invalid(format!("{path}: {err}")))?;
    Ok(Box::new(hocus_day::goa::GoaToken::new(
        bus.to_owned(),
        path,
    )))
}
