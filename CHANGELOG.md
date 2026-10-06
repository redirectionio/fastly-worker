## 3.4.0 - 05-10-2026

* Update libredirectionio to 3.4.0
* Apply the request header filters of the matched rules to the request forwarded to the backend
* Send the tags of the "tag logs" rule action with the logs, the `response_header` variable is resolved from the backend response headers
* Fix the response headers removed by a rule being kept, and only the first value of a repeated response header being filtered

## 3.3.0 - 29-07-2026

* Update libredirectionio to 3.3.0
* Only send the rule-count report when the agent advertises protocol 1.1 or later
* Do not append or prepend text to media and other binary content types

## 2.4.0 - 07-07-2022

 * Fix a bug when multiple rules where used with a backend status code trigger
