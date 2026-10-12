-- The store's schema.
CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL,
  created_at TIMESTAMP
);

CREATE TABLE IF NOT EXISTS public.orders (
  id BIGINT PRIMARY KEY,
  user_id INTEGER REFERENCES users (id),
  total NUMERIC(10, 2)
);

CREATE VIEW active_users AS
  SELECT id, email FROM users WHERE created_at IS NOT NULL;

CREATE MATERIALIZED VIEW reporting.daily_totals AS
  SELECT user_id, SUM(total) AS total FROM public.orders GROUP BY user_id;

CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');

CREATE FUNCTION order_count(uid INTEGER) RETURNS BIGINT AS $$
  SELECT COUNT(*) FROM public.orders WHERE user_id = uid;
$$ LANGUAGE sql;

CREATE OR REPLACE FUNCTION /* by email */ find_user /* one */ (addr TEXT) RETURNS SETOF users AS $$
  SELECT * FROM users WHERE email = addr;
$$ LANGUAGE sql;

CREATE TABLE "audit log" ("when" TIMESTAMP, `who` TEXT);

INSERT INTO users (id, email) VALUES (1, 'a@example.com');
SELECT * FROM users;
