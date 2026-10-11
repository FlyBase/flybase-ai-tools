select n.nspname || '.' || r.relname as relation, has_table_privilege(r.oid, 'select') as ok
from pg_class r join pg_namespace n on n.oid = r.relnamespace
where (n.nspname, r.relname) in (
  ('public','feature'),('public','featureloc'),('public','feature_relationship'),('public','feature_synonym'),('public','synonym'),
  ('public','featureprop'),('public','feature_dbxref'),('public','dbxref'),('public','db'),('public','feature_cvterm'),('public','feature_cvtermprop'),
  ('public','cvterm'),('public','cvtermsynonym'),('public','cvterm_relationship'),('public','cv'),('public','organism'),
  ('public','pub'),('public','pubauthor'),('public','pubprop'),('public','pub_dbxref'),('public','feature_pub'),
  ('public','stock'),('public','stock_genotype'),('public','genotype'),('public','feature_genotype'),('public','stockcollection'),('public','stockcollection_stock'),
  ('public','phenstatement'),('public','phenotype'),('public','phenotype_cvterm'),
  ('public','grp'),('public','humanhealth'),('public','cell_line'),('public','library'),('public','strain'),('mcp','name_index'),
  ('gene','allele'),('gene','allele_class'),('gene','allele_mutagen'),('gene','allele_stock'),('gene','allele_transgenic_product_class'),('gene','insertion'),('gene','construct'),
  ('public','grp_synonym'),('public','grpmember'),('public','feature_grpmember'),('public','grp_relationship'),('public','grpprop'),('public','grp_pub'),('public','grp_cvterm'),
  ('public','humanhealth_synonym'),('public','humanhealthprop'),('public','humanhealth_dbxref'),('public','humanhealth_feature'),('public','humanhealth_featureprop'),('public','humanhealth_pub'),('public','humanhealth_relationship'),('public','humanhealth_cvterm'),
  ('public','cell_line_synonym'),('public','cell_lineprop'),('public','cell_line_pub'),('public','cell_line_cvterm'),('public','cell_line_feature'),
  ('public','strain_synonym'),('public','strainprop'),('public','strain_pub'),
  ('public','library_synonym'),('public','libraryprop'),('public','library_pub'),('public','library_cvterm'),('public','library_feature'),
  ('public','interaction'),('public','feature_interaction'),('public','interaction_pub'),('public','interactionprop'),('public','interaction_cvterm'),
  ('public','feature_relationship_pub'),('public','feature_relationshipprop'),('public','phenotype_comparison'),('public','phenotype_comparison_cvterm'),('public','cvtermprop'),
  ('mcp','cvterm_closure'),('mcp','band_coordinates'),('mcp','aberration_cyto'),
  ('dataclass','diseasev2'),('dataclass_relationship','gene_diseasev2'),('dataclass_relationship','gene_human_orthologv2'),('humanhealth','allele_disease_variant'),
  ('gene','split_system_combination'),('gene','split_system_combination_component_allele'),('gene_group','pathway_member')
)
order by 1
