use std::{
    fmt::{Display, Formatter},
    string::FromUtf8Error,
};

use actix_web::{
    error::ResponseError,
    http::{header::ContentType, StatusCode},
    web::Json,
    HttpResponse,
};
use cloudproof_findex::ser_de::SerializableSetError;
use cosmian_findex::CoreError;

pub type Response<T> = Result<Json<T>, Error>;
pub type ResponseBytes = Result<HttpResponse, Error>;

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "sqlite")]
    Sqlx(sqlx::Error),
    InvalidSignature,
    WrongEncoding,
    Json,
    WrongIndexPublicId,
    Findex(String),

    #[cfg(feature = "rocksdb")]
    Rocksdb(rocksdb::Error),
    #[cfg(feature = "heed")]
    Heed(heed::Error),
    #[cfg(feature = "dynamodb")]
    DynamoDb(String),

    BadRequest(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")?;

        Ok(())
    }
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        log::error!("{self:?}");

        match *self {
            #[cfg(feature = "sqlite")]
            Self::Sqlx(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "dynamodb")]
            Self::DynamoDb(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidSignature => StatusCode::FORBIDDEN,
            Self::WrongEncoding => StatusCode::BAD_REQUEST,
            Self::Json => StatusCode::BAD_REQUEST,
            Self::WrongIndexPublicId => StatusCode::BAD_REQUEST,
            Self::Findex(_) => StatusCode::BAD_REQUEST,

            #[cfg(feature = "rocksdb")]
            Self::Rocksdb(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "heed")]
            Self::Heed(_) => StatusCode::INTERNAL_SERVER_ERROR,

            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Error::Sqlx(err)
    }
}

#[cfg(feature = "rocksdb")]
impl From<rocksdb::Error> for Error {
    fn from(err: rocksdb::Error) -> Self {
        Error::Rocksdb(err)
    }
}

#[cfg(feature = "heed")]
impl From<heed::Error> for Error {
    fn from(err: heed::Error) -> Self {
        Error::Heed(err)
    }
}

#[cfg(feature = "dynamodb")]
impl<T> From<aws_smithy_http::result::SdkError<T>> for Error {
    fn from(err: aws_smithy_http::result::SdkError<T>) -> Self {
        Error::DynamoDb(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Error::Json
    }
}

impl From<FromUtf8Error> for Error {
    fn from(_: FromUtf8Error) -> Self {
        Error::WrongEncoding
    }
}

impl From<CoreError> for Error {
    fn from(err: CoreError) -> Self {
        Error::Findex(err.to_string())
    }
}

impl From<SerializableSetError> for Error {
    fn from(err: SerializableSetError) -> Self {
        Error::Findex(err.to_string())
    }
}
