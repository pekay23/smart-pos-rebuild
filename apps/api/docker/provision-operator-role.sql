DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='pos_operator_runtime') THEN
    CREATE ROLE pos_operator_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
  END IF;
END $$;

SELECT format(
  'ALTER ROLE pos_operator_runtime LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS',
  :'operator_password'
) \gexec
GRANT CONNECT ON DATABASE advanced_pos TO pos_operator_runtime;
GRANT USAGE ON SCHEMA public TO pos_operator_runtime;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='pos_operator_provisioner') THEN
    CREATE ROLE pos_operator_provisioner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
  END IF;
END $$;

SELECT format(
  'ALTER ROLE pos_operator_provisioner LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS',
  :'provisioner_password'
) \gexec
GRANT CONNECT ON DATABASE advanced_pos TO pos_operator_provisioner;
GRANT USAGE ON SCHEMA public TO pos_operator_provisioner;
