// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub(crate) const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityImageType {
    Product,
    Customer,
    Supplier,
    Staff,
}

impl EntityImageType {
    pub(crate) fn parse(value: &str) -> Result<Self, ImageError> {
        match value {
            "product" => Ok(Self::Product),
            "customer" => Ok(Self::Customer),
            "supplier" => Ok(Self::Supplier),
            "staff" => Ok(Self::Staff),
            _ => Err(ImageError::InvalidInput),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Customer => "customer",
            Self::Supplier => "supplier",
            Self::Staff => "staff",
        }
    }

    fn exists_query(self) -> &'static str {
        match self {
            Self::Product => "SELECT EXISTS(SELECT 1 FROM catalog_products WHERE tenant_id=$1::uuid AND id=$2::uuid)",
            Self::Customer => "SELECT EXISTS(SELECT 1 FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2::uuid)",
            Self::Supplier => "SELECT EXISTS(SELECT 1 FROM hosted_suppliers WHERE tenant_id=$1::uuid AND id=$2::uuid)",
            Self::Staff => "SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active')",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageError {
    InvalidInput,
    NotFound,
    Database,
}

pub(crate) struct StoredImage {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

fn validated_content_type(content_type: &str, bytes: &[u8]) -> Result<&'static str, ImageError> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageError::InvalidInput);
    }
    match content_type {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Ok("image/png"),
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Ok("image/jpeg"),
        "image/webp"
            if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" =>
        {
            Ok("image/webp")
        }
        _ => Err(ImageError::InvalidInput),
    }
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<(), ImageError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| ImageError::Database)?;
    }
    Ok(())
}

async fn set_staff_roster_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    entity_type: EntityImageType,
) -> Result<(), ImageError> {
    if entity_type == EntityImageType::Staff {
        sqlx::query("SELECT set_config('pos.staff_roster_tenant_id',$1,true)")
            .bind(tenant_id)
            .execute(&mut **tx)
            .await
            .map_err(|_| ImageError::Database)?;
    }
    Ok(())
}

async fn verify_entity(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    entity_type: EntityImageType,
    entity_id: Uuid,
) -> Result<(), ImageError> {
    let exists = sqlx::query_scalar::<_, bool>(entity_type.exists_query())
        .bind(tenant_id)
        .bind(entity_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ImageError::Database)?;
    if exists {
        Ok(())
    } else {
        Err(ImageError::NotFound)
    }
}

pub(crate) async fn get(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    entity_type: EntityImageType,
    entity_id: &str,
) -> Result<Option<StoredImage>, ImageError> {
    let entity_id = Uuid::parse_str(entity_id).map_err(|_| ImageError::NotFound)?;
    let mut tx = pool.begin().await.map_err(|_| ImageError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    set_staff_roster_context(&mut tx, tenant_id, entity_type).await?;
    verify_entity(&mut tx, tenant_id, entity_type, entity_id).await?;
    let row = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT content_type,image_data FROM hosted_entity_images WHERE tenant_id=$1::uuid AND entity_type=$2 AND entity_id=$3::uuid",
    )
    .bind(tenant_id)
    .bind(entity_type.as_str())
    .bind(entity_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ImageError::Database)?;
    tx.commit().await.map_err(|_| ImageError::Database)?;
    row.map(|(content_type, bytes)| {
        let content_type = match content_type.as_str() {
            "image/jpeg" => "image/jpeg",
            "image/png" => "image/png",
            "image/webp" => "image/webp",
            _ => return Err(ImageError::Database),
        };
        Ok(StoredImage {
            content_type,
            bytes,
        })
    })
    .transpose()
}

pub(crate) async fn put(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    entity_type: EntityImageType,
    entity_id: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), ImageError> {
    let entity_id = Uuid::parse_str(entity_id).map_err(|_| ImageError::NotFound)?;
    let content_type = validated_content_type(content_type, bytes)?;
    let mut tx = pool.begin().await.map_err(|_| ImageError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    set_staff_roster_context(&mut tx, tenant_id, entity_type).await?;
    verify_entity(&mut tx, tenant_id, entity_type, entity_id).await?;
    sqlx::query(
        "INSERT INTO hosted_entity_images(tenant_id,entity_type,entity_id,content_type,byte_length,image_data,created_by_membership_id,updated_by_membership_id) VALUES($1::uuid,$2,$3::uuid,$4,$5,$6,$7::uuid,$7::uuid) ON CONFLICT(tenant_id,entity_type,entity_id) DO UPDATE SET content_type=EXCLUDED.content_type,byte_length=EXCLUDED.byte_length,image_data=EXCLUDED.image_data,updated_by_membership_id=EXCLUDED.updated_by_membership_id,updated_at=now()",
    )
    .bind(tenant_id)
    .bind(entity_type.as_str())
    .bind(entity_id)
    .bind(content_type)
    .bind(bytes.len() as i32)
    .bind(bytes)
    .bind(membership_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ImageError::Database)?;
    audit(
        &mut tx,
        tenant_id,
        membership_id,
        entity_type,
        entity_id,
        "image_uploaded",
        bytes.len(),
    )
    .await?;
    tx.commit().await.map_err(|_| ImageError::Database)
}

pub(crate) async fn delete(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    entity_type: EntityImageType,
    entity_id: &str,
) -> Result<bool, ImageError> {
    let entity_id = Uuid::parse_str(entity_id).map_err(|_| ImageError::NotFound)?;
    let mut tx = pool.begin().await.map_err(|_| ImageError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    set_staff_roster_context(&mut tx, tenant_id, entity_type).await?;
    verify_entity(&mut tx, tenant_id, entity_type, entity_id).await?;
    let deleted = sqlx::query("DELETE FROM hosted_entity_images WHERE tenant_id=$1::uuid AND entity_type=$2 AND entity_id=$3::uuid")
        .bind(tenant_id)
        .bind(entity_type.as_str())
        .bind(entity_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ImageError::Database)?
        .rows_affected() > 0;
    if deleted {
        audit(
            &mut tx,
            tenant_id,
            membership_id,
            entity_type,
            entity_id,
            "image_deleted",
            0,
        )
        .await?;
    }
    tx.commit().await.map_err(|_| ImageError::Database)?;
    Ok(deleted)
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    membership_id: &str,
    entity_type: EntityImageType,
    entity_id: Uuid,
    action: &str,
    byte_length: usize,
) -> Result<(), ImageError> {
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,$3,$4,$5::uuid,jsonb_build_object('byteLength',$6))")
        .bind(tenant_id)
        .bind(membership_id)
        .bind(action)
        .bind(format!("{}_image", entity_type.as_str()))
        .bind(entity_id)
        .bind(byte_length as i64)
        .execute(&mut **tx)
        .await
        .map_err(|_| ImageError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validated_content_type, EntityImageType, ImageError, MAX_IMAGE_BYTES};

    #[test]
    fn accepts_only_bounded_supported_image_signatures() {
        assert_eq!(
            validated_content_type("image/png", b"\x89PNG\r\n\x1a\nbody"),
            Ok("image/png")
        );
        assert_eq!(
            validated_content_type("image/jpeg", &[0xff, 0xd8, 0xff, 0, 1]),
            Ok("image/jpeg")
        );
        assert_eq!(
            validated_content_type("image/webp", b"RIFF0000WEBPdata"),
            Ok("image/webp")
        );
        assert_eq!(
            validated_content_type("image/svg+xml", b"<svg/>"),
            Err(ImageError::InvalidInput)
        );
        assert_eq!(
            validated_content_type("image/png", b"<svg/>"),
            Err(ImageError::InvalidInput)
        );
        assert_eq!(
            validated_content_type("image/png", b""),
            Err(ImageError::InvalidInput)
        );
        assert_eq!(
            validated_content_type("image/png", &vec![0; MAX_IMAGE_BYTES + 1]),
            Err(ImageError::InvalidInput)
        );
    }

    #[test]
    fn entity_type_is_an_exhaustive_allowlist() {
        assert_eq!(
            EntityImageType::parse("product"),
            Ok(EntityImageType::Product)
        );
        assert_eq!(
            EntityImageType::parse("customer"),
            Ok(EntityImageType::Customer)
        );
        assert_eq!(
            EntityImageType::parse("supplier"),
            Ok(EntityImageType::Supplier)
        );
        assert_eq!(EntityImageType::parse("staff"), Ok(EntityImageType::Staff));
        assert_eq!(
            EntityImageType::parse("../products"),
            Err(ImageError::InvalidInput)
        );
    }
}
