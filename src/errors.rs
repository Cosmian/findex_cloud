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
use actix_web_httpauth::{
    extractors::AuthenticationError, headers::www_authenticate::bearer::Bearer,
};
#[cfg(feature = "multitenant")]
use alcoholic_jwt::ValidationError;
use cloudproof_findex::ser_de::SerializableSetError;
use cosmian_findex::CoreError;
#[cfg(feature = "multitenant")]
use reqwest::header::InvalidHeaderValue;

pub type Response<T> = Result<Json<T>, Error>;
pub type ResponseBytes = Result<HttpResponse, Error>;

#[derive(Debug)]
pub enum Error {
    Sqlx(sqlx::Error),
    InvalidSignature,
    WrongEncoding,
    Json,
    WrongIndexPublicId,
    Findex(String),

    #[cfg(feature = "multitenant")]
    InvalidConfiguration,

    #[cfg(feature = "multitenant")]
    CannotFetchJwks(reqwest::Error),
    #[cfg(feature = "multitenant")]
    CannotFetchJwksResponse(reqwest::Error),

    #[cfg(feature = "multitenant")]
    JwksNoKid,
    #[cfg(feature = "multitenant")]
    JwksValidationError(ValidationError),
    #[cfg(feature = "multitenant")]
    TokenKidNotFoundInJwksKeysSet,
    #[cfg(feature = "multitenant")]
    MissingSubInJwtToken,
    #[cfg(feature = "multitenant")]
    InvalidSubInJwtToken,
    #[cfg(feature = "multitenant")]
    TokenExpired,

    #[cfg(feature = "multitenant")]
    FailToBuildBearerHeader(InvalidHeaderValue),
    BearerError(Box<AuthenticationError<Bearer>>),

    #[cfg(feature = "multitenant")]
    UnknownProject(String),

    #[cfg(feature = "multitenant")]
    Reqwest(reqwest::Error),
    #[cfg(feature = "rocksdb")]
    Rocksdb(rocksdb::Error),
    #[cfg(feature = "heed")]
    Heed(heed::Error),

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
            Self::Sqlx(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidSignature => StatusCode::FORBIDDEN,
            Self::WrongEncoding => StatusCode::BAD_REQUEST,
            Self::Json => StatusCode::BAD_REQUEST,
            Self::WrongIndexPublicId => StatusCode::BAD_REQUEST,
            Self::Findex(_) => StatusCode::BAD_REQUEST,

            #[cfg(feature = "multitenant")]
            Self::InvalidConfiguration => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::CannotFetchJwks(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::CannotFetchJwksResponse(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::JwksNoKid => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::JwksValidationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::TokenKidNotFoundInJwksKeysSet => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::MissingSubInJwtToken => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::InvalidSubInJwtToken => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "multitenant")]
            Self::TokenExpired => StatusCode::FORBIDDEN,

            #[cfg(feature = "multitenant")]
            Self::FailToBuildBearerHeader(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::BearerError(_) => StatusCode::FORBIDDEN,

            #[cfg(feature = "multitenant")]
            Self::UnknownProject(_) => StatusCode::NOT_FOUND,
            #[cfg(feature = "multitenant")]
            Self::Reqwest(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "rocksdb")]
            Self::Rocksdb(_) => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "heed")]
            Self::Heed(_) => StatusCode::INTERNAL_SERVER_ERROR,

            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }
}

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

#[cfg(feature = "multitenant")]
impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Reqwest(err)
    }
}

#[cfg(feature = "multitenant")]
impl From<InvalidHeaderValue> for Error {
    fn from(err: InvalidHeaderValue) -> Self {
        Error::FailToBuildBearerHeader(err)
    }
}

impl From<AuthenticationError<Bearer>> for Error {
    fn from(err: AuthenticationError<Bearer>) -> Self {
        Error::BearerError(Box::new(err))
    }
}
