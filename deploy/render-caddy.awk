NR == FNR {
  # CRM credentials are a separate group. All other bcrypt rows belong to panel_admins, which is
  # imported by both panel sites without duplicating secret-bearing lines in the rendered file.
  if ($0 ~ /^\(crm_admins\)/) grp = "crm"
  else if ($0 ~ /^\(/ || $0 ~ /^[a-z]/) grp = ""
  if ($1 != "header_up" && $2 ~ /^\$2/) {
    if (grp == "crm") {
      if (!seen_crm_auth[$0]++) crmauth = crmauth $0 ORS
    } else if (!seen_auth[$0]++) auth = auth $0 ORS
  }
  if ($1 == "header_up" && $2 == "x-api-key") control = $0
  if ($1 == "header_up" && $2 == "x-admin-key") commadmin = $0
  if ($1 == "header_up" && $2 == "x-sales-admin-key") salesadmin = $0
  next
}
/<BASIC_AUTH_USERS_PLACEHOLDER>/ {
  if (auth == "") exit 41
  printf "%s", auth
  auth_used++
  next
}
/<CONTROL_KEY_PLACEHOLDER>/ {
  if (control == "") exit 42
  print control
  control_used++
  next
}
/<ADMIN_CONTROL_KEY_PLACEHOLDER>/ {
  if (control == "") exit 46
  print control
  admin_control_used++
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
/<CRM_ADMIN_USERS_PLACEHOLDER>/ {
  # Bootstrap with a locked credential until operators provision the separate CRM group.
  if (crmauth == "") crmauth = "\t\tdisabled $2a$14$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q" ORS
  printf "%s", crmauth
  crmauth_used++
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
  if (auth_used != 1 || control_used != 1 || admin_control_used != 1 ||
      commadmin_used != 1 || admin_commadmin_used != 1 ||
      crmauth_used != 1 || salesadmin_used != 1) exit 43
}
