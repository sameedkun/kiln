//! Node's release keys ([ADR 0028] §2), copied verbatim from
//! <https://github.com/nodejs/release-keys/tree/main/keys>: current and former releasers, since
//! a release keeps the signature of whoever cut it. `keys/README.md` says how to update them.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use crate::pgp::{self, Key, PgpError};

/// (primary key fingerprint, armored public key), one per releaser.
pub const FILES: &[(&str, &str)] = &[
    (
        "108F52B48DB57BB0CC439B2997B01419BD92F80A",
        include_str!("../keys/108F52B48DB57BB0CC439B2997B01419BD92F80A.asc"),
    ),
    (
        "114F43EE0176B71C7BC219DD50A3051F888C628D",
        include_str!("../keys/114F43EE0176B71C7BC219DD50A3051F888C628D.asc"),
    ),
    (
        "141F07595B7B3FFE74309A937405533BE57C7D57",
        include_str!("../keys/141F07595B7B3FFE74309A937405533BE57C7D57.asc"),
    ),
    (
        "1C050899334244A8AF75E53792EF661D867B9DFA",
        include_str!("../keys/1C050899334244A8AF75E53792EF661D867B9DFA.asc"),
    ),
    (
        "4ED778F539E3634C779C87C6D7062848A1AB005C",
        include_str!("../keys/4ED778F539E3634C779C87C6D7062848A1AB005C.asc"),
    ),
    (
        "56730D5401028683275BD23C23EFEFE93C4CFFFE",
        include_str!("../keys/56730D5401028683275BD23C23EFEFE93C4CFFFE.asc"),
    ),
    (
        "5BE8A3F6C8A5C01D106C0AD820B1A390B168D356",
        include_str!("../keys/5BE8A3F6C8A5C01D106C0AD820B1A390B168D356.asc"),
    ),
    (
        "61FC681DFB92A079F1685E77973F295594EC4689",
        include_str!("../keys/61FC681DFB92A079F1685E77973F295594EC4689.asc"),
    ),
    (
        "655F3B5C1FB3FA8D1A0CA6BDE4A7D232B936D2FD",
        include_str!("../keys/655F3B5C1FB3FA8D1A0CA6BDE4A7D232B936D2FD.asc"),
    ),
    (
        "71DCFD284A79C3B38668286BC97EC7A07EDE3FC1",
        include_str!("../keys/71DCFD284A79C3B38668286BC97EC7A07EDE3FC1.asc"),
    ),
    (
        "74F12602B6F1C4E913FAA37AD3A89613643B6201",
        include_str!("../keys/74F12602B6F1C4E913FAA37AD3A89613643B6201.asc"),
    ),
    (
        "77984A986EBC2AA786BC0F66B01FBB92821C587A",
        include_str!("../keys/77984A986EBC2AA786BC0F66B01FBB92821C587A.asc"),
    ),
    (
        "7937DFD2AB06298B2293C3187D33FF9D0246406D",
        include_str!("../keys/7937DFD2AB06298B2293C3187D33FF9D0246406D.asc"),
    ),
    (
        "890C08DB8579162FEE0DF9DB8BEAB4DFCF555EF4",
        include_str!("../keys/890C08DB8579162FEE0DF9DB8BEAB4DFCF555EF4.asc"),
    ),
    (
        "8FCCA13FEF1D0C2E91008E09770F7A9A5AE15600",
        include_str!("../keys/8FCCA13FEF1D0C2E91008E09770F7A9A5AE15600.asc"),
    ),
    (
        "93C7E9E91B49E432C2F75674B0A78B0A6C481CF6",
        include_str!("../keys/93C7E9E91B49E432C2F75674B0A78B0A6C481CF6.asc"),
    ),
    (
        "94AE36675C464D64BAFA68DD7434390BDBE9B9C5",
        include_str!("../keys/94AE36675C464D64BAFA68DD7434390BDBE9B9C5.asc"),
    ),
    (
        "9554F04D7259F04124DE6B476D5A82AC7E37093B",
        include_str!("../keys/9554F04D7259F04124DE6B476D5A82AC7E37093B.asc"),
    ),
    (
        "A363A499291CBBC940DD62E41F10027AF002F8B0",
        include_str!("../keys/A363A499291CBBC940DD62E41F10027AF002F8B0.asc"),
    ),
    (
        "A48C2BEE680E841632CD4E44F07496B3EB3C1762",
        include_str!("../keys/A48C2BEE680E841632CD4E44F07496B3EB3C1762.asc"),
    ),
    (
        "B9AE9905FFD7803F25714661B63B535A4C206CA9",
        include_str!("../keys/B9AE9905FFD7803F25714661B63B535A4C206CA9.asc"),
    ),
    (
        "B9E2F5981AA6E0CD28160D9FF13993A75599653C",
        include_str!("../keys/B9E2F5981AA6E0CD28160D9FF13993A75599653C.asc"),
    ),
    (
        "C0D6248439F1D5604AAFFB4021D900FFDB233756",
        include_str!("../keys/C0D6248439F1D5604AAFFB4021D900FFDB233756.asc"),
    ),
    (
        "C4F0DFFF4E8C1A8236409D08E73BC641CC11F4C8",
        include_str!("../keys/C4F0DFFF4E8C1A8236409D08E73BC641CC11F4C8.asc"),
    ),
    (
        "C82FA3AE1CBEDC6BE46B9360C43CEC45C17AB93C",
        include_str!("../keys/C82FA3AE1CBEDC6BE46B9360C43CEC45C17AB93C.asc"),
    ),
    (
        "CC68F5A3106FF448322E48ED27F5E38D5B0A215F",
        include_str!("../keys/CC68F5A3106FF448322E48ED27F5E38D5B0A215F.asc"),
    ),
    (
        "DD792F5973C6DE52C432CBDAC77ABFA00DDBF2B7",
        include_str!("../keys/DD792F5973C6DE52C432CBDAC77ABFA00DDBF2B7.asc"),
    ),
    (
        "DD8F2338BAE7501E3DD5AC78C273792F7D83545D",
        include_str!("../keys/DD8F2338BAE7501E3DD5AC78C273792F7D83545D.asc"),
    ),
    (
        "FD3A5288F042B6850C66B31F09FE44734EB7990E",
        include_str!("../keys/FD3A5288F042B6850C66B31F09FE44734EB7990E.asc"),
    ),
];

/// Every key and signing subkey in [`FILES`].
pub fn release_keys() -> Result<Vec<Key>, PgpError> {
    let mut keys = Vec::new();
    for (_, text) in FILES {
        keys.extend(pgp::keys_from_armored(text)?);
    }
    Ok(keys)
}
