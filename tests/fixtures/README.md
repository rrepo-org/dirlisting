# Compatibility fixtures

These are hand-authored, reduced generated-output profiles, not captured server
responses. They retain the identifying containers, headers, navigation, and
record shapes while omitting most styling and scripts. They are part of the
supported compatibility contract, not a claim to cover every server version.

Reference implementations:

* Apache: https://github.com/apache/httpd/blob/2.4.x/modules/generators/mod_autoindex.c
* nginx HTML/JSON/XML: https://github.com/nginx/nginx/blob/master/src/http/modules/ngx_http_autoindex_module.c
* FancyIndex: https://github.com/aperezdc/ngx-fancyindex
* Caddy: https://github.com/caddyserver/caddy/blob/master/modules/caddyhttp/fileserver/browse.html
  and `browse.go` (HTML table profile checked against upstream on 2026-09-24).
* lighttpd: https://github.com/lighttpd/lighttpd1.4/blob/master/src/mod_dirlisting.c

The integration suite also derives empty, malformed, ambiguous, and URL-edge
variants from these profiles. New template support should add its own fixture
and recognition tests.
