-- Run with the release reporting database selected and a headerless batch client.
-- The website validates these three record classes against their report tables.
SELECT line
FROM (
    SELECT 0 AS ord, CONCAT('# release: ', DATABASE()) AS line
    UNION ALL
    SELECT 1, MIN(raw_id)
    FROM (
        SELECT fbid_key AS raw_id FROM stocks__all__main
        UNION ALL
        SELECT fbid_key FROM libraries__all__main
        UNION ALL
        SELECT fbid_key FROM cell_lines__all__main
    ) AS records
    WHERE raw_id IS NOT NULL AND CHAR_LENGTH(TRIM(raw_id)) > 0
    GROUP BY BINARY raw_id
) AS output_rows
ORDER BY ord, BINARY line;
