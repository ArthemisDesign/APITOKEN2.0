# Join root-only aggregate snapshots from commerce and the engine without exporting any identifier.
# File names are passed explicitly because production uses mawk, where GNU awk's ARGIND is absent,
# and an empty provider snapshot must not shift the meaning of later files.
BEGIN { FS = "\t" }

FILENAME == engine_accounts {
  engine_default[$1] = $2
  engine_status[$1] = $3
  next
}

FILENAME == commerce_accounts {
  mapped[$1] = 1
  if (!($1 in engine_default) || engine_default[$1] != $2) default_drift++
  if (!($1 in engine_status) || engine_status[$1] != $3) status_drift++
  next
}

FILENAME == commerce_overrides {
  key = $1 SUBSEP $2
  desired_override[key] = $3
  desired_account[key] = $1
  next
}

FILENAME == engine_overrides {
  if ($1 in mapped) {
    key = $1 SUBSEP $2
    actual_override[key] = $3
    actual_account[key] = $1
  }
  next
}

END {
  for (key in desired_override) {
    if (!(key in actual_override) || desired_override[key] != actual_override[key]) {
      provider_drift_account[desired_account[key]] = 1
    }
  }
  for (key in actual_override) {
    if (!(key in desired_override)) provider_drift_account[actual_account[key]] = 1
  }
  for (account_id in provider_drift_account) provider_drift++
  printf "apitoken_pricing_authority_drift{dimension=\"default\"} %d\n", default_drift
  printf "apitoken_pricing_authority_drift{dimension=\"provider\"} %d\n", provider_drift
  printf "apitoken_pricing_authority_drift{dimension=\"status\"} %d\n", status_drift
  print "apitoken_business_reconciliation_up{scope=\"pricing_authority\"} 1"
}
