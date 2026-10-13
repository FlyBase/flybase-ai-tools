# Wiki API fixtures

Full responses from the public FlyBase wiki API, captured with anonymous GET requests on 2026-10-13 (UTC)
and saved unedited. Tests serve them through wiremock; nothing here is hand-written.

Every URL carries the same parameters the client sends (`format=json&formatversion=2` last).

| File | Content | URL |
|---|---|---|
| `parse_aberration_report.json` | `parse` of FlyBase:Aberration Report (report-guide tables) | <https://wiki.flybase.org/api.php?action=parse&page=FlyBase%3AAberration+Report&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `parse_about.json` | `parse` of FlyBase:About (`dl/dd` address blocks, `<pre>`, external link) | <https://wiki.flybase.org/api.php?action=parse&page=FlyBase%3AAbout&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `parse_batch_download.json` | `parse` of FlyBase:Batch Download | <https://wiki.flybase.org/api.php?action=parse&page=FlyBase%3ABatch+Download&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `parse_balancer_genotype_variant_report.json` | `parse` of FlyBase:Balancer/Genotype variant Report (`/` in the title) | <https://wiki.flybase.org/api.php?action=parse&page=FlyBase%3ABalancer%2FGenotype+variant+Report&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `parse_fbgn0000553.json` | `parse` of FBgn0000553, a broken main-namespace redirect (text `<ol><li>REDIRECT [[...`) | <https://wiki.flybase.org/api.php?action=parse&page=FBgn0000553&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `search_stock.json` | combined search for `stock` (one redirect, FlyBase:Stock Collections to FlyBase:Stocks) | <https://wiki.flybase.org/api.php?action=query&list=search&srsearch=stock&srnamespace=12%7C14%7C1000&srlimit=10&sroffset=0&srprop=snippet%7Ctimestamp&srinfo=totalhits&generator=search&gsrsearch=stock&gsrnamespace=12%7C14%7C1000&gsrlimit=10&gsroffset=0&redirects=1&prop=info&format=json&formatversion=2> |
| `search_report.json` | combined search for `report` (wikitext snippets, a `#REDIRECT` snippet) | <https://wiki.flybase.org/api.php?action=query&list=search&srsearch=report&srnamespace=12%7C14%7C1000&srlimit=10&sroffset=0&srprop=snippet%7Ctimestamp&srinfo=totalhits&generator=search&gsrsearch=report&gsrnamespace=12%7C14%7C1000&gsrlimit=10&gsroffset=0&redirects=1&prop=info&format=json&formatversion=2> |
| `siteinfo_rightsinfo.json` | `siteinfo` `rightsinfo` (both strings empty) | <https://wiki.flybase.org/api.php?action=query&meta=siteinfo&siprop=rightsinfo&format=json&formatversion=2> |
| `error_missingtitle.json` | `parse` of a page that does not exist: `missingtitle` | <https://wiki.flybase.org/api.php?action=parse&page=FlyBase%3ANo+Such+Page+Exists+Here&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `error_invalidtitle.json` | `parse` of `Foo<bar>`: `invalidtitle` | <https://wiki.flybase.org/api.php?action=parse&page=Foo%3Cbar%3E&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `error_pagecannotexist.json` | `parse` of `Special:RecentChanges`: `pagecannotexist` | <https://wiki.flybase.org/api.php?action=parse&page=Special%3ARecentChanges&redirects=1&prop=text%7Csections%7Crevid&disablelimitreport=1&disableeditsection=1&disabletoc=1&format=json&formatversion=2> |
| `search_ftp_archive_zero.json` | combined search for `FTP archive`: zero hits (input to the wildcard retry) | <https://wiki.flybase.org/api.php?action=query&list=search&srsearch=FTP+archive&srnamespace=12%7C14%7C1000&srlimit=10&sroffset=0&srprop=snippet%7Ctimestamp&srinfo=totalhits&generator=search&gsrsearch=FTP+archive&gsrnamespace=12%7C14%7C1000&gsrlimit=10&gsroffset=0&redirects=1&prop=info&format=json&formatversion=2> |
| `search_ftp_archive_wildcard.json` | combined search for `ftp* archive*`: the wildcard retry's hit | <https://wiki.flybase.org/api.php?action=query&list=search&srsearch=ftp%2A+archive%2A&srnamespace=12%7C14%7C1000&srlimit=10&sroffset=0&srprop=snippet%7Ctimestamp&srinfo=totalhits&generator=search&gsrsearch=ftp%2A+archive%2A&gsrnamespace=12%7C14%7C1000&gsrlimit=10&gsroffset=0&redirects=1&prop=info&format=json&formatversion=2> |

To refresh one, fetch its URL again and overwrite the file; keep the bytes as served.
