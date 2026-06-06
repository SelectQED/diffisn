-- ==============================================
-- Analytics Warehouse Schema (v1)
-- ==============================================

CREATE SCHEMA IF NOT EXISTS analytics;

CREATE TABLE analytics.users (
    user_id       BIGINT PRIMARY KEY,
    email         TEXT NOT NULL,
    signup_date   DATE NOT NULL,
    country       TEXT
);

CREATE TABLE analytics.sessions (
    session_id    BIGINT PRIMARY KEY,
    user_id       BIGINT NOT NULL,
    started_at    TIMESTAMPTZ NOT NULL,
    ended_at      TIMESTAMPTZ,
    device_type   TEXT,
    browser       TEXT
);

CREATE TABLE analytics.events (
    event_id      BIGINT PRIMARY KEY,
    session_id    BIGINT NOT NULL,
    event_type    TEXT NOT NULL,
    page_url      TEXT,
    event_time    TIMESTAMPTZ NOT NULL,
    properties    JSONB
);

CREATE TABLE analytics.purchases (
    purchase_id   BIGINT PRIMARY KEY,
    user_id       BIGINT NOT NULL,
    event_id      BIGINT,
    amount_cents  INT NOT NULL,
    currency      TEXT NOT NULL,
    purchase_date TIMESTAMPTZ NOT NULL
);

CREATE TABLE analytics.ref_links (
    link_id       BIGINT PRIMARY KEY,
    campaign      TEXT NOT NULL,
    source        TEXT,
    medium        TEXT,
    clicks        INT DEFAULT 0
);

CREATE TABLE analytics.ab_tests (
    test_id       BIGINT PRIMARY KEY,
    test_name     TEXT NOT NULL,
    variant_a     TEXT NOT NULL,
    variant_b     TEXT NOT NULL,
    start_date    DATE NOT NULL,
    end_date      DATE
);

-- ==============================================
-- Seed data: standard lookup rows (identical)
-- ==============================================

INSERT INTO analytics.users VALUES (1001, 'alice@example.com', '2024-01-15', 'US');
INSERT INTO analytics.users VALUES (1002, 'bob@example.com', '2024-02-20', 'GB');
INSERT INTO analytics.users VALUES (1003, 'carol@example.com', '2024-03-10', 'DE');
INSERT INTO analytics.users VALUES (1004, 'dan@example.com', '2024-04-05', 'US');
INSERT INTO analytics.users VALUES (1005, 'eve@example.com', '2024-04-12', 'FR');
INSERT INTO analytics.users VALUES (1006, 'frank@example.com', '2024-05-01', 'CA');
INSERT INTO analytics.users VALUES (1007, 'grace@example.com', '2024-06-18', 'AU');
INSERT INTO analytics.users VALUES (1008, 'heidi@example.com', '2024-07-22', 'JP');
INSERT INTO analytics.users VALUES (1009, 'ivan@example.com', '2024-08-30', 'BR');
INSERT INTO analytics.users VALUES (1010, 'judy@example.com', '2024-09-14', 'US');

INSERT INTO analytics.sessions VALUES (2001, 1001, '2024-06-01 10:00:00+00', '2024-06-01 10:32:00+00', 'mobile', 'Safari');
INSERT INTO analytics.sessions VALUES (2002, 1002, '2024-06-01 11:15:00+00', '2024-06-01 11:45:00+00', 'desktop', 'Chrome');
INSERT INTO analytics.sessions VALUES (2003, 1003, '2024-06-02 08:00:00+00', '2024-06-02 08:20:00+00', 'desktop', 'Firefox');
INSERT INTO analytics.sessions VALUES (2004, 1004, '2024-06-02 09:30:00+00', '2024-06-02 09:55:00+00', 'tablet', 'Edge');

-- ==============================================
-- Materialized view: daily active users
-- ==============================================

CREATE MATERIALIZED VIEW analytics.daily_active_users AS
SELECT
    started_at::date AS activity_date,
    COUNT(DISTINCT user_id) AS dau,
    COUNT(*) AS session_count
FROM analytics.sessions
GROUP BY started_at::date
ORDER BY activity_date;

-- ==============================================
-- Revenue model: rolling 7-day ARPU
-- ==============================================

CREATE OR REPLACE FUNCTION analytics.rolling_arpu(target_date DATE)
RETURNS TABLE (metric_date DATE, arpu_cents BIGINT) AS $$
BEGIN
    RETURN QUERY
    SELECT
        target_date AS metric_date,
        COALESCE(SUM(p.amount_cents) / NULLIF(COUNT(DISTINCT s.user_id), 0), 0)::BIGINT AS arpu_cents
    FROM analytics.sessions s
    LEFT JOIN analytics.purchases p ON p.user_id = s.user_id
        AND p.purchase_date BETWEEN target_date - INTERVAL '6 days' AND target_date
    WHERE s.started_at::date BETWEEN target_date - INTERVAL '6 days' AND target_date;
END;
$$ LANGUAGE plpgsql;

-- ==============================================
-- FIRST DIFFERENCE: retention model (changed)
-- ==============================================

CREATE OR REPLACE FUNCTION analytics.weekly_retention(start_week DATE)
RETURNS TABLE (week_num INT, retention_pct NUMERIC(5,2)) AS $$
DECLARE
    cohort_size BIGINT;
BEGIN
    SELECT COUNT(DISTINCT user_id) INTO cohort_size
    FROM analytics.sessions
    WHERE started_at::date BETWEEN start_week AND start_week + INTERVAL '6 days';

    FOR week_num IN 0..12 LOOP
        RETURN QUERY
        SELECT
            week_num,
            ROUND(
                100.0 * COUNT(DISTINCT s.user_id) / NULLIF(cohort_size, 0), 2
            ) AS retention_pct
        FROM analytics.sessions s
        WHERE s.user_id IN (
            SELECT DISTINCT user_id
            FROM analytics.sessions
            WHERE started_at::date BETWEEN start_week AND start_week + INTERVAL '6 days'
        )
        AND s.started_at::date BETWEEN start_week + (week_num * INTERVAL '7 days')
                                   AND start_week + (week_num * INTERVAL '7 days') + INTERVAL '6 days';
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- ==============================================
-- SECOND DIFFERENCE: churn model (changed)
-- ==============================================

CREATE OR REPLACE FUNCTION analytics.monthly_churn(target_month DATE)
RETURNS TABLE (month_label TEXT, churn_rate NUMERIC(5,2)) AS $$
DECLARE
    active_prev BIGINT;
    still_active BIGINT;
BEGIN
    SELECT COUNT(DISTINCT user_id) INTO active_prev
    FROM analytics.sessions
    WHERE started_at::date BETWEEN target_month - INTERVAL '1 month'
                               AND target_month - INTERVAL '1 day';

    SELECT COUNT(DISTINCT s.user_id) INTO still_active
    FROM analytics.sessions s
    WHERE s.started_at::date BETWEEN target_month
                                 AND target_month + INTERVAL '1 month' - INTERVAL '1 day'
      AND s.user_id IN (
          SELECT DISTINCT user_id
          FROM analytics.sessions
          WHERE started_at::date BETWEEN target_month - INTERVAL '1 month'
                                     AND target_month - INTERVAL '1 day'
      );

    RETURN QUERY
    SELECT
        to_char(target_month, 'YYYY-MM'),
        ROUND(
            100.0 * (1.0 - still_active::NUMERIC / NULLIF(active_prev, 0)), 2
        ) AS churn_rate;
END;
$$ LANGUAGE plpgsql;
