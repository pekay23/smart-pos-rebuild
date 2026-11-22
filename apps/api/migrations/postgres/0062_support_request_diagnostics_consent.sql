ALTER TABLE hosted_support_requests
    ADD COLUMN diagnostics_consent_at TIMESTAMPTZ,
    ADD COLUMN diagnostics_app_version TEXT,
    ADD COLUMN diagnostics_platform TEXT;

ALTER TABLE hosted_support_requests
    ADD CONSTRAINT hosted_support_request_diagnostics_app_version_check
        CHECK (diagnostics_app_version IS NULL OR diagnostics_app_version ~ '^[A-Za-z0-9][A-Za-z0-9.+-]{0,39}$'),
    ADD CONSTRAINT hosted_support_request_diagnostics_platform_check
        CHECK (diagnostics_platform IS NULL OR diagnostics_platform IN ('web','windows','macos','linux','android','ios','harmonyos')),
    ADD CONSTRAINT hosted_support_request_diagnostics_consent_check
        CHECK (
            (diagnostics_consent_at IS NULL AND diagnostics_app_version IS NULL AND diagnostics_platform IS NULL)
            OR (diagnostics_consent_at IS NOT NULL AND diagnostics_app_version IS NOT NULL AND diagnostics_platform IS NOT NULL)
        );

GRANT SELECT (diagnostics_consent_at,diagnostics_app_version,diagnostics_platform)
    ON hosted_support_requests TO pos_api_runtime,pos_operator_runtime;
GRANT INSERT (diagnostics_consent_at,diagnostics_app_version,diagnostics_platform)
    ON hosted_support_requests TO pos_api_runtime;
