# Error Handling

All API methods can fail and return errors. The error types and handling differ slightly between JavaScript and Rust.

## Error Types (Rust)

The `ApiError` enum has four variants:

```rust
pub enum ApiError {
    /// Network or request error
    Request(String),
    
    /// JSON serialization/deserialization error
    Serialization(String),
    
    /// HTTP error response from the server
    Http {
        status: u16,
        message: String,
        body: Option<String>,
    },
    
    /// Invalid configuration
    Config(String),
}
```

### Request Errors

Network-level failures (connection refused, timeout, DNS failure):

```rust
match client.system().ping().await {
    Err(ApiError::Request(msg)) => {
        println!("Network error: {}", msg);
    }
    _ => {}
}
```

### Serialization Errors

JSON parsing failures (malformed response, type mismatch):

```rust
match client.users().get_current_user().await {
    Err(ApiError::Serialization(msg)) => {
        println!("Failed to parse response: {}", msg);
    }
    _ => {}
}
```

### HTTP Errors

Server returned an error response (4xx or 5xx status):

```rust
match client.users().get_user("invalid-id").await {
    Err(ApiError::Http { status, message, body }) => {
        println!("HTTP {} {}", status, message);
        if let Some(body) = body {
            println!("Response body: {}", body);
        }
    }
    _ => {}
}
```

Common HTTP status codes:

| Status | Meaning |
|--------|---------|
| 400 | Bad Request - Invalid parameters |
| 401 | Unauthorized - Missing or invalid token |
| 403 | Forbidden - Insufficient permissions |
| 404 | Not Found - Resource doesn't exist |
| 429 | Too Many Requests - Rate limited |
| 500 | Internal Server Error |

### Configuration Errors

Invalid client configuration:

```rust
match result {
    Err(ApiError::Config(msg)) => {
        println!("Configuration error: {}", msg);
    }
    _ => {}
}
```

## Error Handling (JavaScript)

In JavaScript, errors are thrown as exceptions with string messages:

```javascript
try {
  const user = await client.users().getCurrentUser();
} catch (error) {
  // error is a string describing the error
  console.error('API Error:', error);
  
  // Check for specific error types
  if (error.includes('HTTP 401')) {
    // Unauthorized - redirect to login
  } else if (error.includes('HTTP 404')) {
    // Not found
  }
}
```

## Best Practices

### 1. Always Handle Errors

```javascript
// Bad - unhandled rejection
const user = await client.users().getCurrentUser();

// Good - catch errors
try {
  const user = await client.users().getCurrentUser();
} catch (error) {
  handleError(error);
}
```

### 2. Handle Authentication Errors

```javascript
async function fetchWithAuth() {
  try {
    return await client.users().getCurrentUser();
  } catch (error) {
    if (error.includes('HTTP 401')) {
      // Token expired or invalid
      await refreshToken();
      return await client.users().getCurrentUser();
    }
    throw error;
  }
}
```

### 3. Provide User Feedback

```javascript
async function loadStudios() {
  try {
    setLoading(true);
    const studios = await client.studios().listStudios();
    setStudios(studios);
  } catch (error) {
    if (error.includes('HTTP 401')) {
      showError('Please log in to view your studios');
    } else if (error.includes('Request error')) {
      showError('Network error. Please check your connection.');
    } else {
      showError('Failed to load studios. Please try again.');
    }
    console.error('API Error:', error);
  } finally {
    setLoading(false);
  }
}
```

### 4. Rust Pattern Matching

```rust
use jacktrip_web::api::{ApiClient, ApiError};

async fn get_user_or_default(client: &ApiClient, id: &str) -> User {
    match client.users().get_user(id).await {
        Ok(user) => user,
        Err(ApiError::Http { status: 404, .. }) => {
            // User not found, return default
            User::default()
        }
        Err(e) => {
            eprintln!("Error fetching user: {}", e);
            User::default()
        }
    }
}
```

