-- Run with the release reporting database selected and a headerless batch client.
-- The website validates these three record classes against their report tables.
SELECT CONCAT('# release: ', DATABASE());
SELECT id
FROM (
    SELECT fbid_key AS id FROM stocks__all__main
    UNION ALL
    SELECT fbid_key FROM libraries__all__main
    UNION ALL
    SELECT fbid_key FROM cell_lines__all__main
) AS records
ORDER BY BINARY id;
