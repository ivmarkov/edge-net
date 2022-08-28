use core::convert::TryInto;
use core::fmt::Debug;

use serde::{Deserialize, Serialize};

use embedded_svc::http::client::*;
use embedded_svc::io::{self, ErrorKind, Io, Read};
use embedded_svc::ota::FirmwareInfo;

use super::json_io;

#[derive(Debug)]
pub enum Error<E> {
    UrlOverflow,
    BufferOverflow,
    FirmwareInfoOverflow,
    Http(E),
}

impl<E> io::Error for Error<E>
where
    E: io::Error,
{
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

// Copied from here:
// https://github.com/XAMPPRocky/octocrab/blob/master/src/models/repos.rs
// To conserve memory, unly the utilized fields are mapped
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Release<'a, const N: usize = 32> {
    pub tag_name: &'a str,
    pub body: Option<&'a str>,
    pub draft: bool,
    pub prerelease: bool,
    // pub created_at: Option<DateTime<Utc>>,
    // pub published_at: Option<DateTime<Utc>>,
    pub assets: heapless::Vec<Asset<'a>, N>,
}

// Copied from here:
// https://github.com/XAMPPRocky/octocrab/blob/master/src/models/repos.rs
// To conserve memory, unly the utilized fields are mapped
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Asset<'a> {
    pub browser_download_url: &'a str,
    pub name: &'a str,
    pub label: Option<&'a str>,
    // pub state: String,
    // pub content_type: String,
    // pub size: i64,
    pub updated_at: &'a str,
    // pub created_at: DateTime<Utc>,
}

impl<'a> Asset<'a> {
    fn as_firmware_info<E>(&'a self, release: &'a Release<'a>) -> Result<FirmwareInfo, Error<E>>
    where
        E: io::Error,
    {
        Ok(FirmwareInfo {
            version: release
                .tag_name
                .try_into()
                .map_err(|_| Error::FirmwareInfoOverflow)?,
            released: self
                .updated_at
                .try_into()
                .map_err(|_| Error::FirmwareInfoOverflow)?,
            description: if let Some(body) = release.body {
                Some(body.try_into().map_err(|_| Error::FirmwareInfoOverflow)?)
            } else {
                None
            },
            signature: None,
            download_id: Some(
                self.browser_download_url
                    .try_into()
                    .map_err(|_| Error::FirmwareInfoOverflow)?,
            ),
        })
    }
}

pub struct GitHubOtaService<'a, C, const B: usize = 1024, const U: usize = 256>
where
    C: Connection,
{
    base_url: heapless::String<U>,
    label: &'a str,
    client: Client<C>,
    buf: [u8; B],
    size: Option<usize>,
}

impl<'a, C, const B: usize, const U: usize> GitHubOtaService<'a, C, B, U>
where
    C: Connection,
{
    pub fn new(base_url: &str, label: &'a str, connection: C) -> Result<Self, Error<C::Error>> {
        Ok(Self {
            base_url: base_url.try_into().map_err(|_| Error::UrlOverflow)?,
            label,
            client: Client::wrap(connection),
            buf: [0_u8; B],
            size: None,
        })
    }

    pub fn new_with_repo(
        repo: &str,
        project: &str,
        label: &'a str,
        client: C,
    ) -> Result<Self, Error<C::Error>> {
        Self::new(
            &join::<U, _>(
                &join::<U, _>("https://api.github.com/repos", repo)?,
                project,
            )?,
            label,
            client,
        )
    }

    pub fn get_latest_release(&mut self) -> Result<Option<FirmwareInfo>, Error<C::Error>> {
        let label = self.label;

        let release = self.get_gh_latest_release()?;

        if let Some(release) = release.as_ref() {
            for asset in &release.assets {
                if asset.label == Some(label) {
                    return Ok(Some(asset.as_firmware_info(release)?));
                }
            }
        }

        Ok(None)
    }

    pub fn get_releases<const N: usize>(
        &mut self,
    ) -> Result<heapless::Vec<FirmwareInfo, N>, Error<C::Error>> {
        let (releases, label) = self.get_gh_releases::<N>()?;

        releases
            .iter()
            .flat_map(|release| {
                release
                    .assets
                    .iter()
                    .filter(|asset| asset.label.as_ref().map(|l| *l == label).unwrap_or(false))
                    .map(move |asset| asset.as_firmware_info(release))
            })
            .collect::<Result<heapless::Vec<_, N>, _>>()
    }

    pub fn open<'b>(&'b mut self, download_id: &'b str) -> Result<&'b mut Self, Error<C::Error>> {
        let response = self
            .client
            .get(download_id)
            .map_err(Error::Http)?
            .submit()
            .map_err(Error::Http)?;

        self.size = response.content_len().map(|n| n as usize);

        Ok(self)
    }

    fn get_gh_releases<const N: usize>(
        &mut self,
    ) -> Result<(heapless::Vec<Release<'_>, N>, &str), Error<C::Error>> {
        let uri = join::<U, _>(&self.base_url, "releases")?;

        let response = self
            .client
            .get(&uri)
            .map_err(Error::Http)?
            .submit()
            .map_err(Error::Http)?;

        let releases =
            json_io::read_buf::<_, heapless::Vec<Release<'_>, N>>(response, &mut self.buf).unwrap(); // TODO

        Ok((releases, self.label))
    }

    fn get_gh_latest_release(&mut self) -> Result<Option<Release<'_>>, Error<C::Error>> {
        let uri = join::<U, _>(&join::<U, _>(&self.base_url, "release")?, "latest")?;

        let response = self
            .client
            .get(&uri)
            .map_err(Error::Http)?
            .submit()
            .map_err(Error::Http)?;

        let release = json_io::read_buf::<_, Option<Release<'_>>>(response, &mut self.buf).unwrap(); // TODO

        Ok(release)
    }
}

impl<'a, C, const B: usize, const U: usize> Io for GitHubOtaService<'a, C, B, U>
where
    C: Connection,
{
    type Error = Error<C::Error>;
}

impl<'a, C, const B: usize, const U: usize> Read for GitHubOtaService<'a, C, B, U>
where
    C: Connection,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.client.connection().read(buf).map_err(Error::Http)
    }
}

fn join<const N: usize, E>(uri: &str, path: &str) -> Result<heapless::String<N>, Error<E>>
where
    E: io::Error,
{
    let uri_slash = uri.ends_with('/');
    let path_slash = path.starts_with('/');

    let uri = if path.is_empty() || path.len() == 1 && uri_slash && path_slash {
        uri.into()
    } else {
        let path = if uri_slash && path_slash {
            &path[1..]
        } else {
            path
        };

        let mut result = heapless::String::from(uri);

        if !uri_slash && !path_slash {
            result.push('/').map_err(|_| Error::UrlOverflow)?;
        }

        result.push_str(path).map_err(|_| Error::UrlOverflow)?;

        result
    };

    Ok(uri)
}

#[cfg(feature = "nightly")]
pub mod asynch {
    use core::convert::TryInto;
    use core::future::Future;

    use embedded_svc::http::client::asynch::*;
    use embedded_svc::io::{asynch::Read, Io};
    use embedded_svc::ota::asynch::FirmwareInfo;

    use crate::utils::json_io::asynch as json_io;

    use super::{join, Release};

    pub use super::Error;

    pub struct GitHubOtaService<'a, C, const B: usize = 1024, const U: usize = 256>
    where
        C: Connection,
    {
        base_url: heapless::String<U>,
        label: &'a str,
        client: Client<C>,
        buf: [u8; B],
        size: Option<usize>,
    }

    impl<'a, C, const B: usize, const U: usize> GitHubOtaService<'a, C, B, U>
    where
        C: Connection,
    {
        pub fn new(base_url: &str, label: &'a str, connection: C) -> Result<Self, Error<C::Error>> {
            Ok(Self {
                base_url: base_url.try_into().map_err(|_| Error::UrlOverflow)?,
                label,
                client: Client::wrap(connection),
                buf: [0_u8; B],
                size: None,
            })
        }

        pub fn new_with_repo(
            repo: &str,
            project: &str,
            label: &'a str,
            client: C,
        ) -> Result<Self, Error<C::Error>> {
            Self::new(
                &join::<U, _>(
                    &join::<U, _>("https://api.github.com/repos", repo)?,
                    project,
                )?,
                label,
                client,
            )
        }

        pub async fn get_latest_release(
            &mut self,
        ) -> Result<Option<FirmwareInfo>, Error<C::Error>> {
            let label = self.label;

            let release = self.get_gh_latest_release().await?;

            if let Some(release) = release.as_ref() {
                for asset in &release.assets {
                    if asset.label == Some(label) {
                        return Ok(Some(asset.as_firmware_info(release)?));
                    }
                }
            }

            Ok(None)
        }

        pub async fn get_releases<const N: usize>(
            &mut self,
        ) -> Result<heapless::Vec<FirmwareInfo, N>, Error<C::Error>> {
            let (releases, label) = self.get_gh_releases::<N>().await?;

            releases
                .iter()
                .flat_map(|release| {
                    release
                        .assets
                        .iter()
                        .filter(|asset| asset.label.as_ref().map(|l| *l == label).unwrap_or(false))
                        .map(move |asset| asset.as_firmware_info(release))
                })
                .collect::<Result<heapless::Vec<_, N>, _>>()
        }

        pub async fn open<'b>(
            &'b mut self,
            download_id: &'b str,
        ) -> Result<&'b mut GitHubOtaService<'a, C, B, U>, Error<C::Error>> {
            let response = self
                .client
                .get(download_id)
                .await
                .map_err(Error::Http)?
                .submit()
                .await
                .map_err(Error::Http)?;

            self.size = response.content_len().map(|n| n as usize);

            Ok(self)
        }

        async fn get_gh_releases<const N: usize>(
            &mut self,
        ) -> Result<(heapless::Vec<Release<'_>, N>, &str), Error<C::Error>> {
            let url = join::<U, _>(&self.base_url, "releases")?;

            let response = self
                .client
                .get(&url)
                .await
                .map_err(Error::Http)?
                .submit()
                .await
                .map_err(Error::Http)?;

            let releases =
                json_io::read_buf::<_, heapless::Vec<Release<'_>, N>>(response, &mut self.buf)
                    .await
                    .unwrap(); // TODO

            Ok((releases, self.label))
        }

        async fn get_gh_latest_release(&mut self) -> Result<Option<Release<'_>>, Error<C::Error>> {
            let url = join::<U, _>(&join::<U, _>(&self.base_url, "release")?, "latest")?;

            let response = self
                .client
                .get(&url)
                .await
                .map_err(Error::Http)?
                .submit()
                .await
                .map_err(Error::Http)?;

            let release = json_io::read_buf::<_, Option<Release<'_>>>(response, &mut self.buf)
                .await
                .unwrap(); // TODO

            Ok(release)
        }
    }

    impl<'a, C, const B: usize, const U: usize> Io for GitHubOtaService<'a, C, B, U>
    where
        C: Connection,
    {
        type Error = Error<C::Error>;
    }

    impl<'a, C, const B: usize, const U: usize> Read for GitHubOtaService<'a, C, B, U>
    where
        C: Connection,
    {
        type ReadFuture<'b> = impl Future<Output = Result<usize, Self::Error>>
        where Self: 'b;

        fn read<'b>(&'b mut self, buf: &'b mut [u8]) -> Self::ReadFuture<'b> {
            async move {
                self.client
                    .connection()
                    .read(buf)
                    .await
                    .map_err(Error::Http)
            }
        }
    }
}
