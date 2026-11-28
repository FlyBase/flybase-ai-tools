-- Direct citations are sufficient for non-gene feature exports. Gene reports
-- also cite synonyms, properties and selected relationships in the FBgn dump.
-- Return one row per publication even if several annotations cite the same paper.
create or replace function mcp.feature_publications(fid integer, gene boolean)
returns table (pub_id integer) language sql stable parallel safe as $$
    with candidates as (
        select fp.pub_id from public.feature_pub fp where fp.feature_id = fid
        union
        select s.pub_id from public.feature_synonym s where gene and s.feature_id = fid
        union
        select pp.pub_id from public.featureprop prop
        join public.featureprop_pub pp using (featureprop_id)
        where gene and prop.feature_id = fid
        union
        select rp.pub_id
        from (
            -- The gene is the subject. The report includes citations on links
            -- to mapping landmarks, constructs, homologs and gene groups.
            select r.feature_relationship_id from public.feature_relationship r
            join public.cvterm t on t.cvterm_id = r.type_id
            join public.feature partner on partner.feature_id = r.object_id and not partner.is_obsolete
            join public.cvterm pt on pt.cvterm_id = partner.type_id
            where gene and r.subject_id = fid and (
                t.name in ('recom_left_end','recom_right_end','maps_to_clone','nomaps_to_clone','identified_with',
                    'fnally_comps','fnally_partcomps','fnally_noncomps','functionally_complements',
                    'member_gene_of','encoded_by','included_in','has_component_gene','putative_ortholog_of',
                    'associated_with','derived_computed_affected_gene')
                or (pt.name in ('transgenic_transposable_element','transposable_element_insertion_site')
                    and t.name in ('derived_assoc_heat_shock_construct','derived_assoc_UAS_construct',
                        'derived_assoc_characterization_construct','derived_assoc_vital_reporter_construct',
                        'derived_assoc_reporter_construct','derived_assoc_insertion_of_mobile_activating_element',
                        'derived_assoc_insertion_of_enhancer_trap','derived_assoc_insertion_of_enhancer_trap_binary_system',
                        'derived_assoc_misc_insertion','derived_assoc_GAL4_reporter_construct')))
            union all
            -- The gene is the object. Only some incoming associations appear
            -- on the report, depending on the partner type and direction.
            select r.feature_relationship_id from public.feature_relationship r
            join public.cvterm t on t.cvterm_id = r.type_id
            join public.feature partner on partner.feature_id = r.subject_id and not partner.is_obsolete
            join public.cvterm pt on pt.cvterm_id = partner.type_id
            where gene and r.object_id = fid and (
                t.name in ('fnally_comps','fnally_partcomps','fnally_noncomps','functionally_complements',
                    'member_gene_of','orthologous_to','paralogous_to','putative_ortholog_of',
                    'useful_Dp_direct','useful_Dp_from_cyto','useful_Df_direct','useful_Df_from_cyto',
                    'deletes','molec_deletes','part_deletes','molec_partdeletes','nondeletes','molec_nondeletes',
                    'duplicates','molec_dups','nonduplicates','molec_nondups','part_duplicates','molec_partdups')
                or (t.name = 'alleleof' and pt.name = 'allele')
                or (t.name = 'associated_with' and pt.name = 'gene' and partner.uniquename ~ '^FBog[0-9]+$')
                or (t.name = 'partof' and not partner.is_analysis
                    and pt.name in ('mRNA','snoRNA','ncRNA','snRNA','tRNA','rRNA','miRNA','pseudogene','pre_miRNA')
                    and exists (select from public.featureloc loc where loc.feature_id = partner.feature_id
                                and loc.srcfeature_id is not null)))
        ) relationships
        join public.feature_relationship_pub rp using (feature_relationship_id)
    )
    select p.pub_id from candidates join public.pub p using (pub_id)
    where case when gene then p.uniquename ~ '^FBrf[0-9]+$' else not p.is_obsolete end
$$;

do $$ begin
  if exists (select 1 from pg_roles where rolname = 'flybase_mcp') then
    grant execute on function mcp.feature_publications(integer, boolean) to flybase_mcp;
  end if;
end $$;
