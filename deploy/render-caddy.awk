NR == FNR {
  if ($1 == "header_up" && $2 == "x-api-key") control = $0
  if ($1 == "header_up" && $2 == "x-admin-key") commadmin = $0
  if ($1 == "header_up" && $2 == "x-sales-admin-key") salesadmin = $0
  next
}
/<ADMIN_CONTROL_KEY_PLACEHOLDER>/ {
  if (control == "") exit 46
  print control
  admin_control_used++
  next
}
/<OPENKEYS_INTERNAL_KEY_PLACEHOLDER>/ {
  if (control == "") exit 49
  openkeyskey = control
  sub(/header_up x-api-key/, "header_up X-OpenKeys-Control-Key", openkeyskey)
  print openkeyskey
  openkeys_internal_used++
  next
}
/<ADMIN_AUTH_KEY_PLACEHOLDER>/ {
  if (commadmin == "") exit 48
  authkey = commadmin
  sub(/header_up x-admin-key/, "header_up X-Admin-Key", authkey)
  print authkey
  authkey_used++
  next
}
/<COMMERCIAL_ADMIN_KEY_PLACEHOLDER>/ {
  if (commadmin == "") exit 44
  print commadmin
  commadmin_used++
  next
}
/<ADMIN_COMMERCIAL_KEY_PLACEHOLDER>/ {
  if (commadmin == "") exit 47
  print commadmin
  admin_commadmin_used++
  next
}
/<SALES_ADMIN_KEY_PLACEHOLDER>/ {
  if (salesadmin == "") exit 45
  print salesadmin
  salesadmin_used++
  next
}
{ print }
END {
  # Control-ключ обслуживает три admin-data upstream'а и переиспользуется закрытым OpenKeys bridge
  # под отдельным header name; остальные service credentials встречаются ровно один раз.
  if (admin_control_used < 1 || openkeys_internal_used != 1 || authkey_used != 1 || commadmin_used != 1 ||
      admin_commadmin_used != 1 || salesadmin_used != 1) exit 43
}
