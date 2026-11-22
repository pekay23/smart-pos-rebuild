CREATE TRIGGER IF NOT EXISTS users_role_business_insert
BEFORE INSERT ON users
WHEN NOT EXISTS (
    SELECT 1 FROM roles r WHERE r.id=NEW.role_id AND r.business_id=NEW.business_id
)
BEGIN
    SELECT RAISE(ABORT, 'user role must belong to the same business');
END;

CREATE TRIGGER IF NOT EXISTS users_role_business_update
BEFORE UPDATE OF role_id,business_id ON users
WHEN NOT EXISTS (
    SELECT 1 FROM roles r WHERE r.id=NEW.role_id AND r.business_id=NEW.business_id
)
BEGIN
    SELECT RAISE(ABORT, 'user role must belong to the same business');
END;

CREATE TRIGGER IF NOT EXISTS assigned_role_business_update
BEFORE UPDATE OF id,business_id ON roles
WHEN EXISTS (
    SELECT 1 FROM users u WHERE u.role_id=OLD.id AND u.business_id<>NEW.business_id
)
BEGIN
    SELECT RAISE(ABORT, 'assigned role business cannot be changed');
END;
