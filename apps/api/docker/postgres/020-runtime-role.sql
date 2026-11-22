CREATE ROLE pos_api_runtime
  LOGIN PASSWORD :'runtime_password'
  NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;

GRANT CONNECT ON DATABASE advanced_pos TO pos_api_runtime;
GRANT USAGE ON SCHEMA public TO pos_api_runtime;

CREATE ROLE pos_operator_runtime
  LOGIN PASSWORD :'operator_password'
  NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;

GRANT CONNECT ON DATABASE advanced_pos TO pos_operator_runtime;
GRANT USAGE ON SCHEMA public TO pos_operator_runtime;
