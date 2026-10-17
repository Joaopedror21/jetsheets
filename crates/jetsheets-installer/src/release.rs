use serde::Deserialize;

pub const REPOSITORY: &str = "Joaopedror21/jetsheets";
pub const ASSET_NAME: &str = "JetSheets-windows-x86_64.exe";
pub const EXECUTABLE_NAME: &str = "JetSheets.exe";
pub const RECORD_NAME: &str = "jetsheets-install.json";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(tag: &str) -> Result<Self, String> {
        let value = tag.strip_prefix('v').unwrap_or(tag);
        let mut parts = value.split('.');
        let number = |part: Option<&str>| -> Result<u64, String> {
            let part = part.ok_or("A versão precisa ter três números.")?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("A tag da versão não é válida (use vMAJOR.MINOR.PATCH).".into());
            }
            part.parse::<u64>().map_err(|error| error.to_string())
        };
        let version = Self(
            number(parts.next())?,
            number(parts.next())?,
            number(parts.next())?,
        );
        if parts.next().is_some() {
            return Err("A tag da versão não é válida (use vMAJOR.MINOR.PATCH).".into());
        }
        Ok(version)
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub notes: String,
    pub page_url: String,
    pub download_url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

impl Release {
    pub fn from_api_json(data: &[u8]) -> Result<Self, String> {
        let api: ApiRelease = serde_json::from_slice(data)
            .map_err(|error| format!("Resposta inválida do GitHub: {error}"))?;
        if api.draft || api.prerelease {
            return Err("A versão encontrada não é uma publicação estável.".into());
        }
        let version = Version::parse(&api.tag_name)?;
        let asset = api
            .assets
            .into_iter()
            .find(|asset| asset.name == ASSET_NAME && asset.state == "uploaded")
            .ok_or("A publicação ainda não contém o executável para Windows.")?;
        if asset.size == 0 || asset.size > 1_000_000_000 {
            return Err("O tamanho do executável publicado é inválido.".into());
        }
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
            .ok_or("O GitHub não informou SHA-256 para este arquivo; instalação recusada.")?;
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("O SHA-256 informado pelo GitHub é inválido.".into());
        }
        let expected_url = format!(
            "https://github.com/{REPOSITORY}/releases/download/{}/{ASSET_NAME}",
            api.tag_name
        );
        if asset.browser_download_url != expected_url {
            return Err("O endereço do executável não corresponde à publicação esperada.".into());
        }
        let expected_page = format!(
            "https://github.com/{REPOSITORY}/releases/tag/{}",
            api.tag_name
        );
        if api.html_url != expected_page {
            return Err("O endereço da publicação não corresponde ao projeto configurado.".into());
        }
        Ok(Self {
            tag: api.tag_name,
            version,
            notes: api.body.unwrap_or_default().chars().take(20_000).collect(),
            page_url: api.html_url,
            download_url: expected_url,
            size: asset.size,
            sha256: sha256.to_ascii_lowercase(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(digest: &str) -> String {
        format!(
            r#"{{"tag_name":"v1.2.3","body":"Correções","html_url":"https://github.com/{REPOSITORY}/releases/tag/v1.2.3","draft":false,"prerelease":false,"assets":[{{"name":"{ASSET_NAME}","state":"uploaded","size":123,"digest":"sha256:{digest}","browser_download_url":"https://github.com/{REPOSITORY}/releases/download/v1.2.3/{ASSET_NAME}"}}]}}"#
        )
    }

    #[test]
    fn validates_release_asset_and_digest() {
        let json = fixture(&"a".repeat(64));
        let release = Release::from_api_json(json.as_bytes()).unwrap();
        assert_eq!(release.version, Version(1, 2, 3));
        assert_eq!(release.size, 123);
        assert!(Release::from_api_json(fixture("invalid").as_bytes()).is_err());
    }

    #[test]
    fn versions_have_numeric_order_and_reject_suffixes() {
        assert!(Version::parse("v1.10.0").unwrap() > Version::parse("v1.9.9").unwrap());
        assert!(Version::parse("v1.0.0-rc1").is_err());
        assert!(Version::parse("v1.0").is_err());
    }
}
