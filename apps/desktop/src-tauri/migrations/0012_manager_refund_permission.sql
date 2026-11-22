UPDATE roles
SET permissions_json = json_insert(permissions_json, '$[#]', 'sales:refund')
WHERE role_key = 'manager'
  AND NOT EXISTS (
      SELECT 1 FROM json_each(roles.permissions_json)
      WHERE json_each.value = 'sales:refund'
  );
