locus_resolve() {
  "$LOCUSCTL" resolve "$@"
}

locus_resolve_all() {
  "$LOCUSCTL" resolve-all "$@"
}

locus_prop_get() {
  "$LOCUSCTL" prop get "$@"
}

locus_props() {
  "$LOCUSCTL" prop list "$@"
}

locus_link_targets() {
  "$LOCUSCTL" link targets "$@"
}

locus_link_sources() {
  "$LOCUSCTL" link sources "$@"
}

locus_context_get() {
  "$LOCUSCTL" context get "$@"
}

locus_context_set() {
  "$LOCUSCTL" context set "$@"
}
