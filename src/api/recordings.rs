//! Recordings API endpoints
//!
//! JackTrip Radio recordings management.

use super::{to_js_value, PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Recordings API
// =============================================================================

api_module_struct!(RecordingsApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

fn recording_like_path(recording_id: &str) -> String {
    format!("/recordings/{}/likes", urlencode(recording_id))
}

impl RecordingsApi {
    /// List all public recordings
    pub async fn list_recordings(&self) -> Result<Vec<models::RecordingMetadata>, ApiError> {
        self.client.get("/recordings").await
    }

    /// List recordings with pagination
    pub async fn list_recordings_paginated(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
        following: Option<bool>,
    ) -> Result<models::PaginatedRecordings, ApiError> {
        #[derive(Serialize)]
        struct Query {
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            following: Option<bool>,
        }

        if page.is_some() || limit.is_some() || following.is_some() {
            self.client.get_with_query("/recordings-paginated", &Query { page, limit, following }).await
        } else {
            self.client.get("/recordings-paginated").await
        }
    }

    /// Get a recording by ID
    pub async fn get_recording(&self, recording_id: &str) -> Result<models::PersonalizedRecording, ApiError> {
        let path = format!("/recordings/{}", urlencode(recording_id));
        self.client.get(&path).await
    }

    /// Get similar recordings
    pub async fn get_similar_recordings(&self, recording_id: &str) -> Result<Vec<models::RecordingMetadata>, ApiError> {
        let path = format!("/recordings/{}/similar", urlencode(recording_id));
        self.client.get(&path).await
    }

    /// Like a recording
    pub async fn like_recording(&self, recording_id: &str) -> Result<(), ApiError> {
        self.client.post_empty_no_response(&recording_like_path(recording_id)).await
    }

    /// Unlike a recording
    pub async fn unlike_recording(&self, recording_id: &str) -> Result<(), ApiError> {
        self.client.delete(&recording_like_path(recording_id)).await
    }

    /// Get recordings for a stream
    pub async fn get_stream_recordings(&self, stream_id: &str) -> Result<Vec<models::RecordingMetadata>, ApiError> {
        let path = format!("/streams/{}/recordings", urlencode(stream_id));
        self.client.get(&path).await
    }

    /// Get all recordings for a studio
    pub async fn get_studio_recordings(&self, studio_id: &str) -> Result<Vec<models::ServerRecording>, ApiError> {
        let path = format!("/studios/{}/recordings", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Get paginated recordings for a studio
    pub async fn get_studio_recordings_paginated(
        &self,
        studio_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedRecordings, ApiError> {
        let path = format!("/studios/{}/recordings-paginated", urlencode(studio_id));

        if page.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &PaginationQuery { page, limit }).await
        } else {
            self.client.get(&path).await
        }
    }

    /// Get a specific recording for a studio
    pub async fn get_studio_recording(
        &self,
        studio_id: &str,
        recording_id: &str,
    ) -> Result<models::ServerRecording, ApiError> {
        let path = format!("/studios/{}/recordings/{}", urlencode(studio_id), urlencode(recording_id));
        self.client.get(&path).await
    }

    /// Update a recording for a studio
    pub async fn update_studio_recording(
        &self,
        studio_id: &str,
        recording_id: &str,
        metadata: &models::RecordingMetadata,
    ) -> Result<models::ServerRecording, ApiError> {
        let path = format!("/studios/{}/recordings/{}", urlencode(studio_id), urlencode(recording_id));
        self.client.put(&path, metadata).await
    }

    /// Delete a recording for a studio
    pub async fn delete_studio_recording(&self, studio_id: &str, recording_id: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}/recordings/{}", urlencode(studio_id), urlencode(recording_id));
        self.client.delete(&path).await
    }

    /// Get the stem summary for a recording
    pub async fn get_recording_stems(
        &self,
        studio_id: &str,
        recording_id: &str,
    ) -> Result<models::StemSummary, ApiError> {
        let path = format!("/studios/{}/recordings/{}/stems", urlencode(studio_id), urlencode(recording_id));
        self.client.get(&path).await
    }

    /// Get all recordings for a user
    pub async fn get_user_recordings(&self, user_id: &str) -> Result<Vec<models::ServerRecording>, ApiError> {
        let path = format!("/users/{}/recordings", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get paginated recordings for a user
    pub async fn get_user_recordings_paginated(
        &self,
        user_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedRecordings, ApiError> {
        let path = format!("/users/{}/recordings-paginated", urlencode(user_id));

        if page.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &PaginationQuery { page, limit }).await
        } else {
            self.client.get(&path).await
        }
    }

    /// Get recordings quota for a user
    pub async fn get_recordings_quota(&self, user_id: &str) -> Result<models::RecordingsQuota, ApiError> {
        let path = format!("/users/{}/recordings/quota", urlencode(user_id));
        self.client.get(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl RecordingsApi {
    #[wasm_bindgen(js_name = listRecordings)]
    pub async fn list_recordings_js(&self) -> Result<JsValue, ApiError> {
        let recordings = self.list_recordings().await?;
        to_js_value(&recordings)
    }

    #[wasm_bindgen(js_name = listRecordingsPaginated)]
    pub async fn list_recordings_paginated_js(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
        following: Option<bool>,
    ) -> Result<models::PaginatedRecordings, ApiError> {
        self.list_recordings_paginated(page, limit, following).await
    }

    #[wasm_bindgen(js_name = getRecording)]
    pub async fn get_recording_js(&self, recording_id: String) -> Result<models::PersonalizedRecording, ApiError> {
        self.get_recording(&recording_id).await
    }

    #[wasm_bindgen(js_name = getSimilarRecordings)]
    pub async fn get_similar_recordings_js(&self, recording_id: String) -> Result<JsValue, ApiError> {
        let recordings = self.get_similar_recordings(&recording_id).await?;
        to_js_value(&recordings)
    }

    #[wasm_bindgen(js_name = likeRecording)]
    pub async fn like_recording_js(&self, recording_id: String) -> Result<(), ApiError> {
        self.like_recording(&recording_id).await
    }

    #[wasm_bindgen(js_name = unlikeRecording)]
    pub async fn unlike_recording_js(&self, recording_id: String) -> Result<(), ApiError> {
        self.unlike_recording(&recording_id).await
    }

    #[wasm_bindgen(js_name = getStreamRecordings)]
    pub async fn get_stream_recordings_js(&self, stream_id: String) -> Result<JsValue, ApiError> {
        let recordings = self.get_stream_recordings(&stream_id).await?;
        to_js_value(&recordings)
    }

    #[wasm_bindgen(js_name = getStudioRecordings)]
    pub async fn get_studio_recordings_js(&self, studio_id: String) -> Result<JsValue, ApiError> {
        let recordings = self.get_studio_recordings(&studio_id).await?;
        to_js_value(&recordings)
    }

    #[wasm_bindgen(js_name = getStudioRecordingsPaginated)]
    pub async fn get_studio_recordings_paginated_js(
        &self,
        studio_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedRecordings, ApiError> {
        self.get_studio_recordings_paginated(&studio_id, page, limit).await
    }

    #[wasm_bindgen(js_name = getStudioRecording)]
    pub async fn get_studio_recording_js(
        &self,
        studio_id: String,
        recording_id: String,
    ) -> Result<models::ServerRecording, ApiError> {
        self.get_studio_recording(&studio_id, &recording_id).await
    }

    #[wasm_bindgen(js_name = updateStudioRecording)]
    pub async fn update_studio_recording_js(
        &self,
        studio_id: String,
        recording_id: String,
        metadata: models::RecordingMetadata,
    ) -> Result<models::ServerRecording, ApiError> {
        self.update_studio_recording(&studio_id, &recording_id, &metadata).await
    }

    #[wasm_bindgen(js_name = deleteStudioRecording)]
    pub async fn delete_studio_recording_js(&self, studio_id: String, recording_id: String) -> Result<(), ApiError> {
        self.delete_studio_recording(&studio_id, &recording_id).await
    }

    #[wasm_bindgen(js_name = getRecordingStems)]
    pub async fn get_recording_stems_js(&self, studio_id: String, recording_id: String) -> Result<models::StemSummary, ApiError> {
        self.get_recording_stems(&studio_id, &recording_id).await
    }

    #[wasm_bindgen(js_name = getUserRecordings)]
    pub async fn get_user_recordings_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let recordings = self.get_user_recordings(&user_id).await?;
        to_js_value(&recordings)
    }

    #[wasm_bindgen(js_name = getUserRecordingsPaginated)]
    pub async fn get_user_recordings_paginated_js(
        &self,
        user_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedRecordings, ApiError> {
        self.get_user_recordings_paginated(&user_id, page, limit).await
    }

    #[wasm_bindgen(js_name = getRecordingsQuota)]
    pub async fn get_recordings_quota_js(&self, user_id: String) -> Result<models::RecordingsQuota, ApiError> {
        self.get_recordings_quota(&user_id).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> RecordingsApi {
        RecordingsApi::from_client(client)
    }

    #[tokio::test]
    async fn test_list_recordings_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/recordings",
            200,
            r#"[{"id":"rec1","name":"Test Recording"}]"#,
        )
        .await;

        let result = api(&client).list_recordings().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, Some("rec1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_recordings_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/recordings", 500, "boom").await;

        let err = api(&client).list_recordings().await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_recordings_paginated_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/recordings-paginated",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":2,"count":1,"limit":10},"results":[{"id":"rec1"}]}"#,
        )
        .await;

        let result = api(&client)
            .list_recordings_paginated(Some(2), Some(10), Some(true))
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.meta.current, 2);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_recordings_paginated_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/recordings-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client)
            .list_recordings_paginated(None, None, None)
            .await
            .unwrap();
        assert!(result.results.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_recording_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/recordings/rec123",
            200,
            r#"{"id":"rec123","name":"My Recording","liked":true}"#,
        )
        .await;

        let recording = api(&client).get_recording("rec123").await.unwrap();
        assert_eq!(recording.metadata.id, Some("rec123".to_string()));
        assert_eq!(recording.metadata.name, Some("My Recording".to_string()));
        assert_eq!(recording.liked, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_recording_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/recordings/nonexistent", 404, "nope").await;

        let err = api(&client).get_recording("nonexistent").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_similar_recordings() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/recordings/rec1/similar",
            200,
            r#"[{"id":"rec2"}]"#,
        )
        .await;

        let result = api(&client).get_similar_recordings("rec1").await.unwrap();
        assert_eq!(result[0].id, Some("rec2".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_like_recording() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "POST", "/recordings/rec1/likes", 204).await;

        api(&client).like_recording("rec1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_unlike_recording() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/recordings/rec1/likes", 204).await;

        api(&client).unlike_recording("rec1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_recordings() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/recordings",
            200,
            r#"[{"id":"rec1"}]"#,
        )
        .await;

        let result = api(&client).get_stream_recordings("s1").await.unwrap();
        assert_eq!(result[0].id, Some("rec1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_recordings() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/recordings",
            200,
            r#"[{"id":"rec1","studioId":"st1"}]"#,
        )
        .await;

        let result = api(&client).get_studio_recordings("st1").await.unwrap();
        assert_eq!(result[0].studio_id, Some("st1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_recordings_paginated_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/recordings-paginated",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":3,"count":1,"limit":5},"results":[{"id":"rec1"}]}"#,
        )
        .await;

        let result = api(&client)
            .get_studio_recordings_paginated("st1", Some(3), Some(5))
            .await
            .unwrap();
        assert_eq!(result.meta.current, 3);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_recordings_paginated_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/recordings-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client)
            .get_studio_recordings_paginated("st1", None, None)
            .await
            .unwrap();
        assert!(result.results.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_recording() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/recordings/rec1",
            200,
            r#"{"id":"rec1","hasStems":true}"#,
        )
        .await;

        let rec = api(&client).get_studio_recording("st1", "rec1").await.unwrap();
        assert_eq!(rec.metadata.id, Some("rec1".to_string()));
        assert_eq!(rec.has_stems, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_studio_recording() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1/recordings/rec1",
            200,
            r#"{"id":"rec1","name":"updated"}"#,
        )
        .await;

        let body = models::RecordingMetadata::default();
        let rec = api(&client)
            .update_studio_recording("st1", "rec1", &body)
            .await
            .unwrap();
        assert_eq!(rec.metadata.name, Some("updated".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_studio_recording() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/recordings/rec1", 204).await;

        api(&client).delete_studio_recording("st1", "rec1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_recording_stems() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/recordings/rec1/stems",
            200,
            r#"{"clients":[{"id":1,"name":"vocals","filename":"stem-1.wav"}]}"#,
        )
        .await;

        let result = api(&client).get_recording_stems("st1", "rec1").await.unwrap();
        let clients = result.clients.expect("clients present");
        assert_eq!(clients[0].name, Some("vocals".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_recordings() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/recordings",
            200,
            r#"[{"id":"rec1"}]"#,
        )
        .await;

        let result = api(&client).get_user_recordings("u1").await.unwrap();
        assert_eq!(result[0].metadata.id, Some("rec1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_recordings_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/u1/recordings", 403, "forbidden").await;

        let err = api(&client).get_user_recordings("u1").await.unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_recordings_paginated_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/recordings-paginated",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":1,"count":1,"limit":20},"results":[{"id":"rec1"}]}"#,
        )
        .await;

        let result = api(&client)
            .get_user_recordings_paginated("u1", Some(1), Some(20))
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_recordings_paginated_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/recordings-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client)
            .get_user_recordings_paginated("u1", None, None)
            .await
            .unwrap();
        assert!(result.results.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_recordings_quota() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/recordings/quota",
            200,
            r#"{"used":1000,"limit":5000,"count":3}"#,
        )
        .await;

        let quota = api(&client).get_recordings_quota("u1").await.unwrap();
        assert_eq!(quota.used, Some(1000));
        assert_eq!(quota.count, Some(3));
        mock.assert_async().await;
    }
}
