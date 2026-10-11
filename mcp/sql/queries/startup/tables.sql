select n.nspname || '.' || r.relname as relation, has_table_privilege(r.oid, 'select') as ok
from pg_class r join pg_namespace n on n.oid = r.relnamespace
where (n.nspname, r.relname) in (
  ('public','feature'),('public','featureloc'),('public','feature_relationship'),('public','feature_synonym'),('public','synonym'),
  ('public','featureprop'),('public','feature_dbxref'),('public','dbxref'),('public','db'),('public','feature_cvterm'),('public','feature_cvtermprop'),
  ('public','cvterm'),('public','cvtermsynonym'),('public','cvterm_relationship'),('public','cv'),('public','organism'),
  ('public','pub'),('public','pubauthor'),('public','pubprop'),('public','pub_dbxref'),('public','feature_pub'),
  ('public','stock'),('public','stock_genotype'),('public','genotype'),('public','feature_genotype'),('public','stockcollection'),('public','stockcollection_stock'),
  ('public','phenstatement'),('public','phenotype'),('public','phenotype_cvterm'),
  ('gene','allele'),('gene','allele_class'),('gene','allele_mutagen'),('gene','allele_stock'),('gene','allele_transgenic_product_class'),('gene','insertion'),('gene','construct')
)
order by 1
