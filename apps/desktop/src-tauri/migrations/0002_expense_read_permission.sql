UPDATE roles
SET permissions_json = replace(
    permissions_json,
    '"expenses:write"',
    '"expenses:read","expenses:write"'
)
WHERE instr(permissions_json, '"expenses:write"') > 0
  AND instr(permissions_json, '"expenses:read"') = 0;
