-- Extra local test databases on the developer commerce Postgres.
-- Production and the watchdog use the same topology: one instance, separate databases.
-- docker-entrypoint-initdb.d runs only on an empty volume. Existing volumes use
-- `deploy/local-test-databases.sh ensure`.
CREATE DATABASE sales;
CREATE DATABASE openkeys;
