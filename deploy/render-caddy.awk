BEGIN {
  if (proxy_admin_key_file == "" || render_output == "") exit 50
  read_status = getline file_proxy_key < proxy_admin_key_file
  if (read_status != 1 || length(file_proxy_key) != 64 || file_proxy_key !~ /^[0-9a-f]+$/) exit 51
  if ((getline extra_line < proxy_admin_key_file) != 0) exit 52
  if (close(proxy_admin_key_file) != 0) exit 52
  key_file_valid = 1
}

NR == FNR {
  if ($1 == "header_up" && $2 == "x-api-key" && control == "") control = $0
  if ($1 == "header_up" && tolower($2) == "x-proxy-admin-key") {
    live_proxy_rows++
    live_proxy_value = $3
    gsub(/^"|"$/, "", live_proxy_value)
  }
  if ($1 == "header_up" && $2 == "x-admin-key") commadmin = $0
  if ($1 == "header_up" && $2 == "x-sales-admin-key") salesadmin = $0
  next
}

{
  template[++template_lines] = $0
  if ($0 ~ /<ADMIN_CONTROL_KEY_PLACEHOLDER>/) admin_control_used++
  if ($0 ~ /<OPENKEYS_INTERNAL_KEY_PLACEHOLDER>/) openkeys_internal_used++
  if ($0 ~ /<ADMIN_AUTH_KEY_PLACEHOLDER>/) authkey_used++
  if ($0 ~ /<COMMERCIAL_ADMIN_KEY_PLACEHOLDER>/) commadmin_used++
  if ($0 ~ /<ADMIN_COMMERCIAL_KEY_PLACEHOLDER>/) admin_commadmin_used++
  if ($0 ~ /<SALES_ADMIN_KEY_PLACEHOLDER>/) salesadmin_used++
  if ($0 ~ /<AUTH_BOT_PROXY_ADMIN_KEY_PLACEHOLDER>/) proxyadmin_used++
}

END {
  if (key_file_valid != 1 || live_proxy_rows > 1 || control == "" || commadmin == "" || salesadmin == "" ||
      admin_control_used < 1 || openkeys_internal_used != 1 || authkey_used != 1 || commadmin_used != 1 ||
      admin_commadmin_used != 1 || salesadmin_used != 1 || proxyadmin_used != 1) exit 43

  if (live_proxy_rows == 1 &&
      (length(live_proxy_value) != 64 || live_proxy_value !~ /^[0-9a-f]+$/ ||
       live_proxy_value != file_proxy_key)) exit 53
  proxyadmin = "\t\t\theader_up X-Proxy-Admin-Key \"" file_proxy_key "\""
  openkeyskey = control
  sub(/header_up x-api-key/, "header_up X-OpenKeys-Control-Key", openkeyskey)
  authkey = commadmin
  sub(/header_up x-admin-key/, "header_up X-Admin-Key", authkey)

  for (line_number = 1; line_number <= template_lines; line_number++) {
    line = template[line_number]
    if (line ~ /<ADMIN_CONTROL_KEY_PLACEHOLDER>/) rendered = control
    else if (line ~ /<OPENKEYS_INTERNAL_KEY_PLACEHOLDER>/) rendered = openkeyskey
    else if (line ~ /<ADMIN_AUTH_KEY_PLACEHOLDER>/) rendered = authkey
    else if (line ~ /<COMMERCIAL_ADMIN_KEY_PLACEHOLDER>/) rendered = commadmin
    else if (line ~ /<ADMIN_COMMERCIAL_KEY_PLACEHOLDER>/) rendered = commadmin
    else if (line ~ /<SALES_ADMIN_KEY_PLACEHOLDER>/) rendered = salesadmin
    else if (line ~ /<AUTH_BOT_PROXY_ADMIN_KEY_PLACEHOLDER>/) rendered = proxyadmin
    else rendered = line
    print rendered > render_output
  }
  if (close(render_output) != 0) exit 54
}
