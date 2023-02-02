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
use cosmian_findex::error::FindexErr;
use hex::FromHexError;
use serde::Serialize;

pub type Response<T> = Result<Json<T>, Error>;
pub type ResponseBytes = Result<HttpResponse, Error>;

#[derive(Debug, Serialize)]
pub enum Error {
    Internal,
    InvalidSignature,
    WrongEncoding,
    Json,
    Hex,
    WrongIndexPublicId,
    Findex(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).map_err(|_| core::fmt::Error)?
        )?;

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
        match *self {
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidSignature => StatusCode::FORBIDDEN,
            Self::WrongEncoding => StatusCode::BAD_REQUEST,
            Self::Json => StatusCode::BAD_REQUEST,
            Self::Hex => StatusCode::BAD_REQUEST,
            Self::WrongIndexPublicId => StatusCode::BAD_REQUEST,
            Self::Findex(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Error::Internal
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

impl From<FromHexError> for Error {
    fn from(_: FromHexError) -> Self {
        Error::Hex
    }
}

impl From<FindexErr> for Error {
    fn from(err: FindexErr) -> Self {
        Error::Findex(err.to_string())
    }
}
