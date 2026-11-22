UPDATE roles
SET permissions_json = (
    SELECT json_group_array(value)
    FROM (
        SELECT value
        FROM json_each(roles.permissions_json)
        WHERE value <> 'sales:read'
        ORDER BY CAST(key AS INTEGER)
    )
)
WHERE role_key = 'cashier';
