# Bundled timezone database

`../tzdata.bin` is a compact archive of TZif files generated from IANA tzdata **2025b**.
The IANA tz database is in the public domain. The archive excludes the `posix/` and `right/`
duplicate trees and metadata files; canonical zone names and compatibility links are retained.

Fusion prefers the host timezone database and falls back to this embedded snapshot. Refresh this
file from a newer IANA tzdata release as part of timezone-data maintenance.
