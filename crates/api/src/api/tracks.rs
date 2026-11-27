//! Backing tracks stored for a studio.
//!
//! Reached through [`ApiClient::tracks`]. Every endpoint requires
//! authentication. Upload, list, and manage the backing-track audio files of a
//! studio (`/studios/{studioId}/tracks*`), modelled by
//! [`models::BackingTrack`].
//!
//! ```rust,no_run
//! use webtrip_api::{api::{ApiClient, ApiError}, models::TrackUpdateRequest};
//!
//! # async fn run(client: &ApiClient, wav_bytes: Vec<u8>) -> Result<(), ApiError> {
//! let track = client.tracks().upload_track("studio123", "bassline.wav", wav_bytes).await?;
//! if let Some(id) = &track.id {
//!     let update = TrackUpdateRequest { name: Some("Bass line".into()) };
//!     client.tracks().update_track("studio123", id, &update).await?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ```js
//! const track = await client.tracks().uploadTrack('studio123', 'bassline.wav', wavBytes); // Uint8Array
//! await client.tracks().updateTrack('studio123', track.id, { name: 'Bass line' });
//! const { results } = await client.tracks().listTracks('studio123', 1, 20);
//! ```
//!
//! Downloading a track's audio is not available here: that endpoint is for
//! studio servers and authenticates with server credentials, not a user token.

use super::{ApiClient, ApiError, PaginationQuery, urlencode};
use crate::models;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

api_module_struct!(
    /// Studio backing-track endpoints; obtain with [`ApiClient::tracks`].
    TracksApi
);

fn tracks_path(studio_id: &str) -> String {
    format!("/studios/{}/tracks", urlencode(studio_id))
}

fn track_path(studio_id: &str, track_id: &str) -> String {
    format!("/studios/{}/tracks/{}", urlencode(studio_id), urlencode(track_id))
}

impl TracksApi {
    /// List backing tracks for a studio (`GET /studios/{studioId}/tracks`).
    ///
    /// `page` and `limit` are sent only when given; the server applies its own
    /// defaults otherwise.
    pub async fn list_tracks(
        &self,
        studio_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedBackingTracks, ApiError> {
        self.client.get_with_query(&tracks_path(studio_id), &PaginationQuery { page, limit }).await
    }

    /// Upload a new backing track (`POST /studios/{studioId}/tracks`).
    ///
    /// `data` is the raw WAV, MP3, or FLAC file, sent as the `file` part of a
    /// `multipart/form-data` body under `file_name`.
    ///
    /// # Errors
    ///
    /// [`ApiError::Http`] `406` if the file is not WAV, MP3 or FLAC.
    pub async fn upload_track(
        &self,
        studio_id: &str,
        file_name: &str,
        data: Vec<u8>,
    ) -> Result<models::BackingTrack, ApiError> {
        let part = reqwest::multipart::Part::bytes(data).file_name(file_name.to_string());
        let form = reqwest::multipart::Form::new().part("file", part);
        self.client.post_multipart(&tracks_path(studio_id), form).await
    }

    /// Get a backing track by ID (`GET /studios/{studioId}/tracks/{trackId}`).
    pub async fn get_track(&self, studio_id: &str, track_id: &str) -> Result<models::BackingTrack, ApiError> {
        self.client.get(&track_path(studio_id, track_id)).await
    }

    /// Rename a backing track (`PUT /studios/{studioId}/tracks/{trackId}`).
    pub async fn update_track(
        &self,
        studio_id: &str,
        track_id: &str,
        update: &models::TrackUpdateRequest,
    ) -> Result<models::BackingTrack, ApiError> {
        self.client.put(&track_path(studio_id, track_id), update).await
    }

    /// Delete a backing track (`DELETE /studios/{studioId}/tracks/{trackId}`).
    pub async fn delete_track(&self, studio_id: &str, track_id: &str) -> Result<(), ApiError> {
        self.client.delete(&track_path(studio_id, track_id)).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[cfg(feature = "wasm")]
#[wasm_bindgen]
impl TracksApi {
    #[wasm_bindgen(js_name = listTracks)]
    pub async fn list_tracks_js(
        &self,
        studio_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedBackingTracks, ApiError> {
        self.list_tracks(&studio_id, page, limit).await
    }

    #[wasm_bindgen(js_name = uploadTrack)]
    pub async fn upload_track_js(
        &self,
        studio_id: String,
        file_name: String,
        data: Vec<u8>,
    ) -> Result<models::BackingTrack, ApiError> {
        self.upload_track(&studio_id, &file_name, data).await
    }

    #[wasm_bindgen(js_name = getTrack)]
    pub async fn get_track_js(&self, studio_id: String, track_id: String) -> Result<models::BackingTrack, ApiError> {
        self.get_track(&studio_id, &track_id).await
    }

    #[wasm_bindgen(js_name = updateTrack)]
    pub async fn update_track_js(
        &self,
        studio_id: String,
        track_id: String,
        update: models::TrackUpdateRequest,
    ) -> Result<models::BackingTrack, ApiError> {
        self.update_track(&studio_id, &track_id, &update).await
    }

    #[wasm_bindgen(js_name = deleteTrack)]
    pub async fn delete_track_js(&self, studio_id: String, track_id: String) -> Result<(), ApiError> {
        self.delete_track(&studio_id, &track_id).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> TracksApi {
        TracksApi::from_client(client)
    }

    #[tokio::test]
    async fn test_list_tracks_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/tracks",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":2,"count":1,"limit":10},"results":[{"id":"trk1","serverId":"st1"}]}"#,
        )
        .await;

        let result = api(&client).list_tracks("st1", Some(2), Some(10)).await.unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].id, Some("trk1".to_string()));
        assert_eq!(result.meta.current, 2);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_tracks_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/tracks",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).list_tracks("st1", None, None).await.unwrap();
        assert!(result.results.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_tracks_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/st1/tracks", 403, "Forbidden").await;

        let err = api(&client).list_tracks("st1", None, None).await.unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_upload_track_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios/st1/tracks",
            201,
            r#"{"id":"trk1","serverId":"st1","name":"loop.wav","status":0}"#,
        )
        .await;

        let track = api(&client)
            .upload_track("st1", "loop.wav", b"RIFF....WAVE".to_vec())
            .await
            .unwrap();
        assert_eq!(track.id, Some("trk1".to_string()));
        assert_eq!(track.name, Some("loop.wav".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_upload_track_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/studios/st1/tracks", 406, "Not Acceptable").await;

        let err = api(&client)
            .upload_track("st1", "loop.txt", b"not audio".to_vec())
            .await
            .unwrap_err();
        assert_http_status(err, 406);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_track_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/tracks/trk1",
            200,
            r#"{"id":"trk1","serverId":"st1","name":"Drum Loop","duration":120,"status":0}"#,
        )
        .await;

        let track = api(&client).get_track("st1", "trk1").await.unwrap();
        assert_eq!(track.id, Some("trk1".to_string()));
        assert_eq!(track.duration, Some(120));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_track_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/st1/tracks/nope", 404, "Not Found").await;

        let err = api(&client).get_track("st1", "nope").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_track_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1/tracks/trk1",
            200,
            r#"{"id":"trk1","serverId":"st1","name":"Renamed Loop","status":0}"#,
        )
        .await;

        let body = models::TrackUpdateRequest {
            name: Some("Renamed Loop".to_string()),
            ..Default::default()
        };
        let track = api(&client).update_track("st1", "trk1", &body).await.unwrap();
        assert_eq!(track.name, Some("Renamed Loop".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_track_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "PUT", "/studios/st1/tracks/trk1", 400, "Bad Request").await;

        let body = models::TrackUpdateRequest::default();
        let err = api(&client).update_track("st1", "trk1", &body).await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_track_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/tracks/trk1", 204).await;

        api(&client).delete_track("st1", "trk1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_track_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/tracks/trk1", 404).await;

        let err = api(&client).delete_track("st1", "trk1").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }
}
