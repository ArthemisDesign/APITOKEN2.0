function emit(username, password_hash, domains) {
  if (username !~ /^[A-Za-z0-9._@-]+$/ || length(username) > 80) invalid = 1
  if (password_hash !~ /^\$2[aby]\$[0-9][0-9]\$[.\/A-Za-z0-9]+$/ || length(password_hash) != 60) invalid = 1
  if (invalid) return
  if (rows++) printf ",\n"
  printf "    {\"username\":\"%s\",\"password_hash\":\"%s\",\"domains\":%s}", username, password_hash, domains
}

BEGIN {
  print "{\"accounts\":["
}

/^\(panel_admins\)[[:space:]]*\{/ {
  group = "panel"
  next
}

/^\(crm_admins\)[[:space:]]*\{/ {
  group = "crm"
  next
}

/^\(/ {
  group = ""
}

$1 ~ /\./ && $2 == "{" {
  group = ""
}

group != "" && $2 ~ /^\$2[aby]\$/ {
  if ($1 == "disabled") next
  if (group == "panel") {
    emit($1, $2, "[\"admin.apitoken.sale\",\"admin.partners.apitoken.sale\",\"content-studio.apitoken.sale\",\"monitoring.apitoken.sale\"]")
    panel_rows++
  } else if (group == "crm") {
    emit($1, $2, "[\"crm.apitoken.sale\"]")
    crm_rows++
  }
}

END {
  print "\n]}"
  if (invalid) exit 42
  if (panel_rows < 1 || crm_rows < 1) exit 43
}
