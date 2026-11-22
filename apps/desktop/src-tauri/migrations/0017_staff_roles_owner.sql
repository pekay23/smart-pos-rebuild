UPDATE roles
SET workspace_mode='cashier'
WHERE role_key='cashier';

INSERT OR IGNORE INTO roles(id,business_id,role_key,display_name,permissions_json,is_system,workspace_mode,role_revision)
SELECT 'owner:'||b.id,b.id,'owner','Owner',
       '["pos:use","products:read","products:write","inventory:write","purchases:read","purchases:write","sales:read","sales:refund","customers:read","customers:write","suppliers:read","suppliers:write","expenses:read","expenses:write","reports:read","settings:write","users:write","license:manage","backups:manage","roles:manage"]',
       1,'management',1
FROM businesses b
WHERE EXISTS (SELECT 1 FROM roles r WHERE r.business_id=b.id AND r.role_key='admin');

UPDATE businesses
SET owner_user_id=COALESCE(
    (SELECT e.user_id
     FROM audit_events e JOIN users u ON u.id=e.user_id
     WHERE e.business_id=businesses.id AND e.action IN ('store_initialized','demo_workspace_created') AND u.is_active=1
     ORDER BY CASE e.action WHEN 'store_initialized' THEN 0 ELSE 1 END,e.created_at,e.id
     LIMIT 1), owner_user_id
)
WHERE owner_user_id IS NULL;

UPDATE users
SET role_id=(SELECT r.id FROM roles r WHERE r.business_id=users.business_id AND r.role_key='owner')
WHERE id IN (SELECT owner_user_id FROM businesses WHERE owner_user_id IS NOT NULL)
  AND EXISTS (SELECT 1 FROM roles r WHERE r.business_id=users.business_id AND r.role_key='owner');

UPDATE roles
SET permissions_json=(
    SELECT json_group_array(value)
    FROM json_each(roles.permissions_json)
    WHERE value NOT IN ('license:manage','backups:manage','roles:manage')
)
WHERE role_key='admin' AND is_system=1;
