#!/usr/bin/env bash
# Exports a schema-only dump of public chado plus rows for the fixture IDs and their neighbours into fixture/data/ (git-ignored).
#
# Constraints of the public server, and how this script works within them:
# - The role has a 2-minute statement_timeout, and the server also drops connections whose query runs much past
#   two minutes even after SET statement_timeout = 0. Every copy below is sized to finish well inside that.
# - The role has no TEMP privilege and psql -c cannot mix SQL with \copy, so the feature-id set is computed
#   client-side and each \copy is its own psql session.
# - Id lists are passed as one array parameter, = any(string_to_array('...', ',')::int[]), not as IN lists,
#   and the statement goes to psql on stdin: a list of tens of thousands of ids exceeds the kernel's
#   128 KB per-argument limit for -c.
set -euo pipefail
cd "$(dirname "$0")"
SRC="${CHADO_PUBLIC_URL:-host=chado.flybase.org port=5432 dbname=flybase user=flybase}"
mkdir -p data
NOTIMEOUT="set statement_timeout = 0"
q() { psql "$SRC" -v ON_ERROR_STOP=1 -qAt -c "$NOTIMEOUT" -c "$1"; }
# cp_ <query> <name>: copies the query result to data/<name>.tsv and logs row count and elapsed seconds. \copy must be one line.
cp_() {
  local t0=$SECONDS
  printf '%s\n' "$NOTIMEOUT;" "\\copy ($(echo "$1" | tr '\n' ' ')) to 'data/$2.tsv'" | psql "$SRC" -v ON_ERROR_STOP=1 -qAt
  echo "$2: $(wc -l < "data/$2.tsv") rows, $((SECONDS - t0))s"
}
# in_ <column> <comma-separated ids>: the array-parameter membership test.
in_() { echo "$1 = any(string_to_array('$2', ',')::int[])"; }

t0=$SECONDS
pg_dump "$SRC" --schema-only --no-owner --no-privileges -f data/schema.sql
echo "schema: $((SECONDS - t0))s"

IDS=$(paste -sd, ids.txt | sed "s/\([^,]*\)/'\1'/g")
SO="(select cv_id from cv where name = 'SO')"
GP="(select cvterm_id from cvterm where name = 'golden_path' and cv_id = $SO)"
# Feature-id set (reduced from the plan's generic two-hop rule, which reached 208k features and 8.8M relationships
# because Notch alone has 2,305 direct neighbours):
#   seeds
#   + hop1: every feature one relationship away from a seed (either direction), and the seeds' srcfeatures
#   + transcript children: subjects of the transcripts (FBtr) of seed genes (FBgn), i.e. their exons and polypeptides
#   + all Dmel chromosome bands
#   + physical-interaction partners: the -XP polypeptides of seed genes, their interaction partners, and the partners' genes
#   + polypeptide domains located on seed polypeptides (FBpp)
#   + the first 5,000 clones of library FBlc0000001 (its library_feature sample below).
# Neighbours of hop1 features that are not seed-gene transcripts (alleles' constructs, other genes' alleles, ...) are not included.
t0=$SECONDS
FIDS=$(q "with seed as (select feature_id, uniquename from feature where uniquename in ($IDS)),
 hop1 as (select feature_id from seed
   union select subject_id from feature_relationship where object_id in (select feature_id from seed)
   union select object_id from feature_relationship where subject_id in (select feature_id from seed)
   union select srcfeature_id from featureloc where feature_id in (select feature_id from seed)),
 seed_tx as (select fr.subject_id as feature_id from feature_relationship fr join feature t on t.feature_id = fr.subject_id
   where fr.object_id in (select feature_id from seed where uniquename like 'FBgn%') and t.uniquename like 'FBtr%'),
 tx_children as (select subject_id as feature_id from feature_relationship where object_id in (select feature_id from seed_tx)),
 xp as (select s.feature_id from seed g join feature_relationship fr on fr.object_id = g.feature_id
   join feature s on s.feature_id = fr.subject_id where g.uniquename like 'FBgn%' and s.name like '%-XP'),
 partners as (select fo.feature_id from xp join feature_interaction fi on fi.feature_id = xp.feature_id
   join feature_interaction fo on fo.interaction_id = fi.interaction_id where fo.feature_id <> xp.feature_id
   union select fr.object_id from feature_relationship fr where fr.subject_id in (
     select fo.feature_id from xp join feature_interaction fi on fi.feature_id = xp.feature_id
     join feature_interaction fo on fo.interaction_id = fi.interaction_id where fo.feature_id <> xp.feature_id)),
 domains as (select fl.feature_id from featureloc fl where fl.srcfeature_id in (select feature_id from seed where uniquename like 'FBpp%')),
 clones as (select lf.feature_id from library_feature lf where lf.library_id = (select library_id from library where uniquename = 'FBlc0000001') order by lf.library_feature_id limit 5000),
 bands as (select feature_id from feature where type_id = (select cvterm_id from cvterm where name = 'chromosome_band' and cv_id = $SO)
   and organism_id = (select organism_id from organism where abbreviation = 'Dmel'))
 select string_agg(feature_id::text, ',') from (select feature_id from hop1 union select feature_id from tx_children union select feature_id from bands
   union select feature_id from partners union select feature_id from domains union select feature_id from clones) x")
echo "feature ids: $(echo "$FIDS" | tr ',' '\n' | wc -l), $((SECONDS - t0))s"

# Feature rows, excluding golden paths (their residues are 20+ Mb each; a selected copy follows).
cp_ "select * from feature where $(in_ feature_id "$FIDS") and type_id <> $GP" feature
# Golden paths, whole: Dmel X, 2L and 3R (about 79 Mb of residues; the sequence tests read X:3.13 Mb, X:21.5 Mb,
# 2L:4.5 Mb and 3R:26.6 Mb), plus the Dpse scaffold 3_groupMISC, which has NULL residues and seqlen in chado.
# The other Dmel arms (2R, 3L, 4, Y) are copied with NULL residues and their true seqlen so mcp.band_coordinates
# covers every arm; get_sequence on them answers "has no stored sequence".
cp_ "select * from feature where type_id = $GP
     and ((uniquename in ('X', '2L', '3R') and organism_id = (select organism_id from organism where abbreviation = 'Dmel'))
       or (uniquename = '3_groupMISC' and organism_id = (select organism_id from organism where abbreviation = 'Dpse')))
     union all
     select feature_id, dbxref_id, organism_id, name, uniquename, null, seqlen, md5checksum, type_id, is_analysis, timeaccessioned, timelastmodified, is_obsolete
     from feature where type_id = $GP and uniquename in ('2R', '3L', '4', 'Y') and organism_id = (select organism_id from organism where abbreviation = 'Dmel')" zz_feature_golden_path
cp_ "select * from featureloc where $(in_ feature_id "$FIDS")" featureloc
cp_ "select * from feature_relationship where $(in_ subject_id "$FIDS") and $(in_ object_id "$FIDS")" feature_relationship
cp_ "select * from feature_synonym where $(in_ feature_id "$FIDS")" feature_synonym
cp_ "select * from synonym where synonym_id in (select synonym_id from feature_synonym where $(in_ feature_id "$FIDS")
     union select synonym_id from cell_line_synonym union select synonym_id from strain_synonym union select synonym_id from library_synonym
     union select synonym_id from grp_synonym union select synonym_id from humanhealth_synonym)" synonym
cp_ "select * from featureprop where $(in_ feature_id "$FIDS")" featureprop
cp_ "select * from feature_dbxref where $(in_ feature_id "$FIDS")" feature_dbxref
cp_ "select * from feature_cvterm where $(in_ feature_id "$FIDS")" feature_cvterm
cp_ "select * from feature_cvtermprop where feature_cvterm_id in (select feature_cvterm_id from feature_cvterm where $(in_ feature_id "$FIDS"))" feature_cvtermprop
cp_ "select * from feature_pub where $(in_ feature_id "$FIDS")" feature_pub
cp_ "select * from feature_genotype where $(in_ feature_id "$FIDS")" feature_genotype
cp_ "select * from phenstatement where genotype_id in (select genotype_id from feature_genotype where $(in_ feature_id "$FIDS"))" phenstatement

# Whole small tables.
for t in organism cv cvterm cvtermsynonym cvterm_relationship db pub pubauthor pubprop pub_dbxref stock stock_genotype genotype stockcollection stockcollection_stock \
         phenotype phenotype_cvterm grp grpmember grp_synonym grpprop humanhealth humanhealth_synonym humanhealthprop \
         cell_line cell_line_synonym strain strain_synonym library library_synonym; do
  cp_ "select * from $t" "$t"
done
for t in dataclass.genev2 dataclass.allelev2 dataclass.diseasev2 dataclass_relationship.gene_allelev2 dataclass_relationship.gene_human_orthologv2 \
         gene.allele gene.allele_stock gene.allele_class gene.allele_mutagen gene.allele_transgenic_product_class gene.insertion gene.construct; do
  cp_ "select * from $t" "${t//./__}"
done

# Plan 2 tables: whole copies for the small ones; the relationship and interaction side tables need both ends
# (or the interaction) inside the feature-id set, like feature_relationship above.
for t in feature_grpmember grp_relationship grp_pub grp_cvterm \
         humanhealth_dbxref humanhealth_feature humanhealth_featureprop humanhealth_pub humanhealth_relationship humanhealth_cvterm \
         cell_lineprop cell_line_pub cell_line_cvterm cell_line_feature strainprop strain_pub \
         libraryprop library_pub library_cvterm cvtermprop phenotype_comparison_cvterm; do
  cp_ "select * from $t" "$t"
done
cp_ "select * from library_feature where library_id = (select library_id from library where uniquename = 'FBlc0000001') order by library_feature_id limit 5000" library_feature
FR="(select feature_relationship_id from feature_relationship where $(in_ subject_id "$FIDS") and $(in_ object_id "$FIDS"))"
cp_ "select * from feature_relationship_pub where feature_relationship_id in $FR" feature_relationship_pub
cp_ "select * from feature_relationshipprop where feature_relationship_id in $FR" feature_relationshipprop
cp_ "select * from feature_interaction where $(in_ feature_id "$FIDS")" feature_interaction
FI="(select interaction_id from feature_interaction where $(in_ feature_id "$FIDS"))"
cp_ "select * from interaction where interaction_id in $FI" interaction
cp_ "select * from interaction_pub where interaction_id in $FI" interaction_pub
cp_ "select * from interactionprop where interaction_id in $FI" interactionprop
cp_ "select * from interaction_cvterm where interaction_id in $FI" interaction_cvterm
cp_ "select * from phenotype_comparison where genotype1_id in (select genotype_id from feature_genotype where $(in_ feature_id "$FIDS"))" phenotype_comparison
for t in dataclass_relationship.gene_diseasev2 humanhealth.allele_disease_variant \
         gene.split_system_combination gene.split_system_combination_component_allele gene_group.pathway_member; do
  cp_ "select * from $t" "${t//./__}"
done

# dbxref: only the rows referenced by the exported feature, golden path, feature_dbxref, pub_dbxref and cvterm rows.
# The ids are taken from the TSVs already written (dbxref_id is column 2 of feature, 3 of feature_dbxref and pub_dbxref,
# 4 of cvterm) and copied in chunks so each query stays far below the server's two-minute limit.
{ cut -f2 data/feature.tsv data/zz_feature_golden_path.tsv; cut -f3 data/feature_dbxref.tsv data/pub_dbxref.tsv; cut -f4 data/cvterm.tsv; } \
  | grep -E '^[0-9]+$' | sort -un > data/dbxref_ids.txt
echo "dbxref ids: $(wc -l < data/dbxref_ids.txt)"
t0=$SECONDS
: > data/dbxref.tsv
split -l 100000 data/dbxref_ids.txt data/dbxref_ids.chunk.
for c in data/dbxref_ids.chunk.*; do
  printf '%s\n' "$NOTIMEOUT;" "\\copy (select * from dbxref where $(in_ dbxref_id "$(paste -sd, "$c")")) to stdout" \
    | psql "$SRC" -v ON_ERROR_STOP=1 -qAt >> data/dbxref.tsv
done
rm -f data/dbxref_ids.chunk.*
echo "dbxref: $(wc -l < data/dbxref.tsv) rows, $((SECONDS - t0))s"
echo "export complete"
